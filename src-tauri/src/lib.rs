use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Credentials;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tokio::io::AsyncWriteExt;
use std::io::Write;
use tauri::{AppHandle, Emitter};
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
    job_id: String,
    bucket: String,
    prefix: String,
    destination: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    job_id: String,
    operation: String,
    status: String,
    key: String,
    file_name: String,
    size: i64,
    discovered: usize,
    completed: usize,
    error: Option<String>,
}

fn emit_download_progress(app: &AppHandle, progress: DownloadProgress) {
    let _ = app.emit("download-progress", progress);
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
        files.extend(output.contents().iter().filter_map(|object| object.key().and_then(|key| {
            if key.ends_with('/') { return None; }
            Some(FolderFile { key: key.to_owned(), name: relative_key(key, &prefix), size: object.size() })
        })));
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
    let mut prefixes = BTreeSet::new();
    loop {
        let mut request = client.list_objects_v2().bucket(bucket.trim()).prefix(prefix);
        if let Some(value) = token { request = request.continuation_token(value); }
        let output = request.send().await.map_err(|error| format!("Unable to list folder '{}': {error}", prefix))?;
        for item in output.contents() {
            if let Some(key) = item.key() {
                if key.ends_with('/') {
                    prefixes.insert(key.to_owned());
                } else {
                    objects.push((key.to_owned(), item.size().unwrap_or_default()));
                    let relative = relative_key(key, prefix);
                    let mut current = String::new();
                    let parts = relative.split('/').collect::<Vec<_>>();
                    for part in parts.iter().take(parts.len().saturating_sub(1)) {
                        current.push_str(part);
                        current.push('/');
                        prefixes.insert(format!("{}{}", prefix, current));
                    }
                }
            }
        }
        token = output.next_continuation_token().map(str::to_owned);
        if token.is_none() { break; }
    }
    Ok((objects, prefixes.into_iter().collect()))
}

fn relative_key(key: &str, prefix: &str) -> String { key.strip_prefix(prefix).unwrap_or(key).trim_start_matches('/').to_owned() }

fn safe_relative_components(value: &str) -> Result<Vec<String>, String> {
    let mut components = Vec::new();
    for component in value.split('/') {
        // A leading slash in an S3 key is part of the key, not an absolute
        // local path. Empty path components are therefore ignored.
        if component.is_empty() { continue; }

        let mut local_name = String::new();
        for character in component.chars() {
            // Backslashes and other Windows path characters are valid S3 key
            // characters, but must not become local path syntax. Percent
            // encoding keeps the download safe and deterministic.
            if matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || character.is_control() {
                local_name.push_str(&format!("%{:02X}", character as u32));
            } else {
                local_name.push(character);
            }
        }
        if local_name == "." { local_name = "%2E".to_string(); }
        if local_name == ".." { local_name = "%2E%2E".to_string(); }
        if local_name.ends_with('.') || local_name.ends_with(' ') {
            local_name = local_name.trim_end_matches(['.', ' ']).to_string() + "%20";
        }
        components.push(local_name);
    }
    if components.is_empty() { return Err("S3 key resolved to an empty relative path.".to_string()); }
    Ok(components)
}

fn safe_relative_path(value: &str) -> Result<std::path::PathBuf, String> {
    Ok(safe_relative_components(value)?.into_iter().collect())
}

fn safe_zip_path(value: &str) -> Result<String, String> {
    Ok(safe_relative_components(value)?.join("/"))
}

fn folder_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_matches('/');
    if trimmed.is_empty() { String::new() } else { format!("{trimmed}/") }
}

fn download_root(destination: &str, bucket: &str, prefix: &str) -> Result<std::path::PathBuf, String> {
    let name = prefix.trim_end_matches('/').rsplit('/').next().filter(|value| !value.is_empty()).unwrap_or(bucket.trim());
    Ok(std::path::PathBuf::from(destination).join(safe_relative_path(name)?))
}

#[tauri::command]
async fn download_s3_folder(app: AppHandle, request: DownloadFolderRequest) -> Result<(), String> {
    let client = s3_client().await?;
    let prefix = folder_prefix(&request.prefix);
    let (objects, _) = list_folder_objects(&client, &request.bucket, &prefix).await?;
    let root = download_root(&request.destination, &request.bucket, &prefix)?;
    tokio::fs::create_dir_all(&root).await.map_err(|error| format!("Unable to create destination folder: {error}"))?;
    let mut discovered = 0;
    let mut completed = 0;
    for (key, size) in objects {
        discovered += 1;
        let file_name = relative_key(&key, &prefix);
        let progress = |status: &str, completed_count: usize, error: Option<String>| emit_download_progress(&app, DownloadProgress {
            job_id: request.job_id.clone(), operation: "folder".to_string(), status: status.to_string(),
            key: key.clone(), file_name: file_name.clone(), size, discovered, completed: completed_count, error,
        });
        progress("discovered", completed, None);
        let relative = safe_relative_path(&relative_key(&key, &prefix))?;
        let target = root.join(relative);
        progress("downloading", completed, None);
        let result = async {
            if let Some(parent) = target.parent() { tokio::fs::create_dir_all(parent).await.map_err(|error| format!("Unable to create destination folder: {error}"))?; }
            let response = client.get_object().bucket(request.bucket.trim()).key(&key).send().await.map_err(|error| format!("Unable to download '{}': {error}", key))?;
            Ok::<_, String>(response)
        }.await;
        let response = match result {
            Ok(response) => response,
            Err(error) => { progress("failed", completed, Some(error)); continue; }
        };
        let mut file = match tokio::fs::File::create(&target).await {
            Ok(file) => file,
            Err(error) => { progress("failed", completed, Some(format!("Unable to create '{}': {error}", target.display()))); continue; }
        };
        let mut stream = response.body.into_async_read();
        if let Err(error) = tokio::io::copy(&mut stream, &mut file).await {
            let _ = tokio::fs::remove_file(&target).await;
            progress("failed", completed, Some(format!("Unable to write '{}': {error}", target.display())));
            continue;
        }
        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&target).await;
            progress("failed", completed, Some(format!("Unable to finish '{}': {error}", target.display())));
            continue;
        }
        completed += 1;
        progress("completed", completed, None);
    }
    Ok(())
}

#[tauri::command]
async fn download_s3_folder_zip(app: AppHandle, request: DownloadFolderRequest) -> Result<(), String> {
    let client = s3_client().await?;
    let prefix = folder_prefix(&request.prefix);
    let (objects, prefixes) = list_folder_objects(&client, &request.bucket, &prefix).await?;
    let destination = std::path::PathBuf::from(&request.destination);
    let file = std::fs::File::create(&destination).map_err(|error| format!("Unable to create ZIP archive: {error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    for folder in prefixes {
        let relative = relative_key(&folder, &prefix).trim_end_matches('/').to_owned();
        if !relative.is_empty() {
            let safe_relative = match safe_zip_path(&relative) {
                Ok(path) => path,
                Err(_) => {
                    let _ = std::fs::remove_file(&destination);
                    return Err(format!("S3 key contains an unsafe relative path: '{folder}'"));
                }
            };
            if let Err(error) = archive.add_directory(format!("{safe_relative}/"), SimpleFileOptions::default()) {
                let _ = std::fs::remove_file(&destination);
                return Err(format!("Unable to add folder to ZIP: {error}"));
            }
        }
    }
    let mut discovered = 0;
    let mut completed = 0;
    for (key, size) in objects {
        let relative = relative_key(&key, &prefix);
        discovered += 1;
        let progress = |status: &str, completed_count: usize, error: Option<String>| emit_download_progress(&app, DownloadProgress {
            job_id: request.job_id.clone(), operation: "zip".to_string(), status: status.to_string(),
            key: key.clone(), file_name: relative.clone(), size, discovered, completed: completed_count, error,
        });
        progress("discovered", completed, None);
        let safe_relative = match safe_zip_path(&relative) {
            Ok(path) => path,
            Err(_) => {
                progress("failed", completed, Some(format!("S3 key contains an unsafe relative path: '{key}'")));
                continue;
            }
        };
        progress("downloading", completed, None);
        let response = match client.get_object().bucket(request.bucket.trim()).key(&key).send().await {
            Ok(response) => response,
            Err(error) => { progress("failed", completed, Some(format!("Unable to download '{key}': {error}"))); continue; }
        };
        let bytes = match response.body.collect().await {
            Ok(bytes) => bytes.into_bytes(),
            Err(error) => { progress("failed", completed, Some(format!("Unable to read '{key}': {error}"))); continue; }
        };
        if let Err(error) = archive.start_file(&safe_relative, SimpleFileOptions::default()) {
            progress("failed", completed, Some(format!("Unable to add '{key}' to ZIP: {error}")));
            continue;
        }
        if let Err(error) = archive.write_all(&bytes) {
            progress("failed", completed, Some(format!("Unable to write ZIP archive: {error}")));
            continue;
        }
        completed += 1;
        progress("completed", completed, None);
    }
    if let Err(error) = archive.finish() {
        let _ = std::fs::remove_file(&destination);
        return Err(format!("Unable to finish ZIP archive: {error}"));
    }
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
