use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Credentials;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use std::io::Write;
use zip::write::SimpleFileOptions;

const CREDENTIAL_SERVICE: &str = "S3 Browser";
const CREDENTIAL_USER: &str = "default";

fn default_region() -> String {
    "eu-central-1".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionRequest {
    access_key_id: String,
    secret_access_key: String,
    region: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedSettings {
    access_key_id: String,
    secret_access_key: String,
    #[serde(default = "default_region")]
    region: String,
    bucket: String,
    prefix: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSettingsRequest {
    access_key_id: String,
    secret_access_key: String,
    region: String,
    bucket: String,
    prefix: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BucketSummary {
    name: String,
    creation_date: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionResponse {
    success: bool,
    region: String,
    buckets: Vec<BucketSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListObjectsRequest {
    bucket: String,
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadObjectRequest {
    bucket: String,
    key: String,
    destination: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadFolderRequest {
    bucket: String,
    prefix: String,
    destination: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderFile {
    key: String,
    name: String,
    size: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct S3Entry {
    key: String,
    name: String,
    kind: String,
    size: Option<i64>,
    last_modified: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListObjectsResponse {
    bucket: String,
    prefix: String,
    entries: Vec<S3Entry>,
}

fn credential_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|error| format!("Unable to access secure credential storage: {error}"))
}

#[tauri::command]
fn load_saved_settings() -> Result<Option<SavedSettings>, String> {
    let entry = credential_entry()?;
    match entry.get_password() {
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|error| format!("Saved settings are invalid: {error}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Unable to load saved settings: {error}")),
    }
}

#[tauri::command]
fn save_settings(settings: SaveSettingsRequest) -> Result<(), String> {
    let value = serde_json::to_string(&settings)
        .map_err(|error| format!("Unable to prepare saved settings: {error}"))?;
    credential_entry()?
        .set_password(&value)
        .map_err(|error| format!("Unable to save settings securely: {error}"))
}

#[tauri::command]
async fn connect_to_s3(request: ConnectionRequest) -> Result<ConnectionResponse, String> {
    let credentials = Credentials::new(
        request.access_key_id.trim(),
        request.secret_access_key.trim(),
        None,
        None,
        "S3 Browser",
    );
    let region = request.region.trim().to_owned();
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_types::region::Region::new(region.clone()))
        .credentials_provider(credentials)
        .load()
        .await;
    let client = aws_sdk_s3::Client::new(&config);
    let output = client
        .list_buckets()
        .send()
        .await
        .map_err(|_| "AWS rejected the connection or the identity cannot list buckets.".to_string())?;

    Ok(ConnectionResponse {
        success: true,
        region,
        buckets: output
            .buckets()
            .iter()
            .filter_map(|bucket| bucket.name().map(|name| BucketSummary {
                name: name.to_owned(),
                creation_date: bucket.creation_date().map(|date| date.to_string()),
            }))
            .collect(),
    })
}

#[tauri::command]
async fn list_s3_objects(request: ListObjectsRequest) -> Result<ListObjectsResponse, String> {
    let settings = load_saved_settings()?
        .ok_or_else(|| "No saved S3 credentials were found. Connect again to continue.".to_string())?;
    let credentials = Credentials::new(
        settings.access_key_id.trim(),
        settings.secret_access_key.trim(),
        None,
        None,
        "S3 Browser",
    );
    let region = settings.region.trim().to_owned();
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_types::region::Region::new(region))
        .credentials_provider(credentials)
        .load()
        .await;
    let client = aws_sdk_s3::Client::new(&config);
    let prefix = request.prefix.trim().to_owned();
    let output = client
        .list_objects_v2()
        .bucket(request.bucket.trim())
        .prefix(&prefix)
        .delimiter("/")
        .send()
        .await
        .map_err(|error| {
            let detail = format!("{error:?}");
            if detail.contains("AccessDenied") || detail.contains("Forbidden") {
                format!("Unable to list this S3 location: AWS denied s3:ListBucket for bucket '{}' (prefix '{}'). Add s3:ListBucket permission for this bucket. Details: {detail}", request.bucket, prefix)
            } else if detail.contains("PermanentRedirect") || detail.contains("AuthorizationHeaderMalformed") || detail.contains("IncorrectRegion") {
                format!("Unable to list this S3 location: the bucket is in a different AWS region than the selected region. Details: {detail}")
            } else {
                format!("Unable to list this S3 location: {detail}")
            }
        })?;

    let mut entries = output
        .common_prefixes()
        .iter()
        .filter_map(|common_prefix| common_prefix.prefix())
        .map(|key| S3Entry {
            key: key.to_owned(),
            name: key.strip_prefix(&prefix).unwrap_or(key).trim_end_matches('/').to_owned(),
            kind: "folder".to_string(),
            size: None,
            last_modified: None,
        })
        .collect::<Vec<_>>();
    entries.extend(output.contents().iter().filter_map(|object| {
        let key = object.key()?;
        if key == prefix {
            return None;
        }
        Some(S3Entry {
            key: key.to_owned(),
            name: key.strip_prefix(&prefix).unwrap_or(key).to_owned(),
            kind: "file".to_string(),
            size: Some(object.size().unwrap_or_default()),
            last_modified: object.last_modified().map(|date| date.to_string()),
        })
    }));
    entries.sort_by_key(|entry| (entry.kind != "folder", entry.name.to_lowercase()));

    Ok(ListObjectsResponse { bucket: request.bucket, prefix, entries })
}

#[tauri::command]
async fn download_s3_object(request: DownloadObjectRequest) -> Result<(), String> {
    let settings = load_saved_settings()?
        .ok_or_else(|| "No saved S3 credentials were found. Connect again to continue.".to_string())?;
    let credentials = Credentials::new(
        settings.access_key_id.trim(),
        settings.secret_access_key.trim(),
        None,
        None,
        "S3 Browser",
    );
    let region = settings.region.trim().to_owned();
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_types::region::Region::new(region))
        .credentials_provider(credentials)
        .load()
        .await;
    let client = aws_sdk_s3::Client::new(&config);
    let destination = std::path::PathBuf::from(&request.destination);
    let response = client
        .get_object()
        .bucket(request.bucket.trim())
        .key(&request.key)
        .send()
        .await
        .map_err(|error| format!("Unable to download '{}': {error}", request.key))?;

    let mut file = tokio::fs::File::create(&destination)
        .await
        .map_err(|error| format!("Unable to create the destination file: {error}"))?;
    let mut stream = response.body.into_async_read();
    if let Err(error) = tokio::io::copy(&mut stream, &mut file).await {
        let _ = tokio::fs::remove_file(&destination).await;
        return Err(format!("Unable to write the downloaded file: {error}"));
    }
    file.flush().await.map_err(|error| format!("Unable to finish the downloaded file: {error}"))?;
    Ok(())
}

#[tauri::command]
async fn list_s3_folder_files(request: ListObjectsRequest) -> Result<Vec<FolderFile>, String> {
    let client = s3_client().await?;
    let prefix = if request.prefix.ends_with('/') { request.prefix } else { format!("{}/", request.prefix) };
    let mut token = None;
    let mut files = Vec::new();
    loop {
        let mut query = client.list_objects_v2().bucket(request.bucket.trim()).prefix(&prefix);
        if let Some(value) = token { query = query.continuation_token(value); }
        let output = query.send().await.map_err(|error| format!("Unable to list folder '{}': {error}", prefix))?;
        files.extend(output.contents().iter().filter_map(|object| object.key().map(|key| FolderFile { key: key.to_owned(), name: relative_key(key, &prefix), size: object.size() })));
        token = output.next_continuation_token().map(str::to_owned);
        if token.is_none() { break; }
    }
    files.sort_by_key(|file| file.key.to_lowercase());
    Ok(files)
}

async fn s3_client() -> Result<aws_sdk_s3::Client, String> {
    let settings = load_saved_settings()?.ok_or_else(|| "No saved S3 credentials were found. Connect again to continue.".to_string())?;
    let credentials = Credentials::new(settings.access_key_id.trim(), settings.secret_access_key.trim(), None, None, "S3 Browser");
    let config = aws_config::defaults(BehaviorVersion::latest()).region(aws_types::region::Region::new(settings.region.trim().to_owned())).credentials_provider(credentials).load().await;
    Ok(aws_sdk_s3::Client::new(&config))
}

async fn list_folder_objects(client: &aws_sdk_s3::Client, bucket: &str, prefix: &str) -> Result<(Vec<(String, i64)>, Vec<String>), String> {
    let mut token = None;
    let mut objects = Vec::new();
    let mut prefixes = Vec::new();
    loop {
        let mut request = client.list_objects_v2().bucket(bucket.trim()).prefix(prefix).delimiter("/");
        if let Some(value) = token { request = request.continuation_token(value); }
        let output = request.send().await.map_err(|error| format!("Unable to list folder '{}': {error}", prefix))?;
        prefixes.extend(output.common_prefixes().iter().filter_map(|item| item.prefix().map(str::to_owned)));
        objects.extend(output.contents().iter().filter_map(|item| item.key().map(|key| (key.to_owned(), item.size().unwrap_or_default()))));
        token = output.next_continuation_token().map(str::to_owned);
        if token.is_none() { break; }
    }
    Ok((objects, prefixes))
}

fn relative_key(key: &str, prefix: &str) -> String { key.strip_prefix(prefix).unwrap_or(key).trim_start_matches('/').to_owned() }

#[tauri::command]
async fn download_s3_folder(request: DownloadFolderRequest) -> Result<(), String> {
    let client = s3_client().await?;
    let prefix = if request.prefix.ends_with('/') { request.prefix.clone() } else { format!("{}/", request.prefix) };
    let (objects, _) = list_folder_objects(&client, &request.bucket, &prefix).await?;
    let root = std::path::PathBuf::from(&request.destination).join(prefix.trim_end_matches('/').rsplit('/').next().unwrap_or("folder"));
    tokio::fs::create_dir_all(&root).await.map_err(|error| format!("Unable to create destination folder: {error}"))?;
    for (key, _) in objects {
        let target = root.join(relative_key(&key, &prefix));
        if let Some(parent) = target.parent() { tokio::fs::create_dir_all(parent).await.map_err(|error| format!("Unable to create destination folder: {error}"))?; }
        let response = client.get_object().bucket(request.bucket.trim()).key(&key).send().await.map_err(|error| format!("Unable to download '{}': {error}", key))?;
        let mut file = tokio::fs::File::create(&target).await.map_err(|error| format!("Unable to create '{}': {error}", target.display()))?;
        let mut stream = response.body.into_async_read();
        tokio::io::copy(&mut stream, &mut file).await.map_err(|error| format!("Unable to write '{}': {error}", target.display()))?;
        file.flush().await.map_err(|error| format!("Unable to finish '{}': {error}", target.display()))?;
    }
    Ok(())
}

#[tauri::command]
async fn download_s3_folder_zip(request: DownloadFolderRequest) -> Result<(), String> {
    let client = s3_client().await?;
    let prefix = if request.prefix.ends_with('/') { request.prefix.clone() } else { format!("{}/", request.prefix) };
    let (objects, prefixes) = list_folder_objects(&client, &request.bucket, &prefix).await?;
    let destination = std::path::PathBuf::from(&request.destination);
    let file = std::fs::File::create(&destination).map_err(|error| format!("Unable to create ZIP archive: {error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    for folder in prefixes { archive.add_directory(format!("{}/", relative_key(&folder, &prefix)), SimpleFileOptions::default()).map_err(|error| format!("Unable to add folder to ZIP: {error}"))?; }
    for (key, _) in objects { let response = client.get_object().bucket(request.bucket.trim()).key(&key).send().await.map_err(|error| format!("Unable to download '{}': {error}", key))?; let bytes = response.body.collect().await.map_err(|error| format!("Unable to read '{}': {error}", key))?.into_bytes(); archive.start_file(relative_key(&key, &prefix), SimpleFileOptions::default()).map_err(|error| format!("Unable to add '{}' to ZIP: {error}", key))?; archive.write_all(&bytes).map_err(|error| format!("Unable to write ZIP archive: {error}"))?; }
    archive.finish().map_err(|error| format!("Unable to finish ZIP archive: {error}"))?;
    Ok(())
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            load_saved_settings,
            save_settings,
            connect_to_s3,
            list_s3_objects,
            list_s3_folder_files,
            download_s3_object
            ,download_s3_folder
            ,download_s3_folder_zip
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
