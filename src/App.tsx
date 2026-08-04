import { FormEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import "./App.css";

type S3Credentials = { accessKeyId: string; secretAccessKey: string; region: string; bucket: string; prefix: string };
type BucketSummary = { name: string; creationDate?: string };
type ConnectionResponse = { success: boolean; region: string; buckets: BucketSummary[] };
type S3Entry = { key: string; name: string; kind: "folder" | "file"; size?: number; lastModified?: string };
type ListObjectsResponse = { bucket: string; prefix: string; entries: S3Entry[] };
type Location = { bucket: string; prefix: string };
type DownloadStatus = "queued" | "downloading" | "completed" | "failed";
type DownloadItem = { id: string; bucket: string; name: string; key: string; destination: string; size?: number; status: DownloadStatus; error?: string; kind?: "file" | "zip" };
type FolderFile = { key: string; name: string; size?: number };

const regions = [
  ["us-east-1", "US East (N. Virginia)"], ["us-west-2", "US West (Oregon)"],
  ["eu-central-1", "Europe (Frankfurt)"], ["eu-west-1", "Europe (Ireland)"],
  ["ap-southeast-1", "Asia Pacific (Singapore)"],
];

function App() {
  const [credentials, setCredentials] = useState<S3Credentials>({ accessKeyId: "", secretAccessKey: "", region: "eu-central-1", bucket: "", prefix: "" });
  const [showSecret, setShowSecret] = useState(false);
  const [error, setError] = useState("");
  const [isConnecting, setIsConnecting] = useState(false);
  const [connection, setConnection] = useState<ConnectionResponse | null>(null);
  const [location, setLocation] = useState<Location | null>(null);
  const [entries, setEntries] = useState<S3Entry[]>([]);
  const [history, setHistory] = useState<Location[]>([]);
  const [selectedKey, setSelectedKey] = useState("");
  const [isLoadingObjects, setIsLoadingObjects] = useState(false);
  const [browserError, setBrowserError] = useState("");
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [showDownloads, setShowDownloads] = useState(false);
  const processingDownloads = useRef(false);
  const downloadingKey = "";
  const downloadingFolderKey = "";

  useEffect(() => {
    void invoke<S3Credentials | null>("load_saved_settings").then((saved) => {
      if (saved) setCredentials({ ...saved, region: saved.region || "eu-central-1" });
    }).catch(() => undefined);
  }, []);

  function updateField(field: keyof S3Credentials, value: string) {
    setCredentials((current) => ({ ...current, [field]: value }));
    setError("");
    setConnection(null);
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!credentials.accessKeyId.trim() || !credentials.secretAccessKey.trim()) {
      setError("Enter both your access key ID and secret access key to continue.");
      return;
    }
    setError(""); setIsConnecting(true);
    try {
      const result = await invoke<ConnectionResponse>("connect_to_s3", { request: { accessKeyId: credentials.accessKeyId, secretAccessKey: credentials.secretAccessKey, region: credentials.region } });
      await invoke("save_settings", { settings: credentials });
      setConnection(result);
      const defaultBucket = credentials.bucket.trim();
      if (defaultBucket) openBucket(defaultBucket);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Unable to connect to Amazon S3.");
    } finally { setIsConnecting(false); }
  }

  async function loadLocation(next: Location) {
    setLocation(next);
    setEntries([]);
    setIsLoadingObjects(true); setBrowserError(""); setSelectedKey("");
    try {
      const result = await invoke<ListObjectsResponse>("list_s3_objects", { request: next });
      setLocation({ bucket: result.bucket, prefix: result.prefix });
      setEntries(result.entries);
    } catch (reason) {
      setBrowserError(typeof reason === "string" ? reason : "Unable to load this S3 location.");
    } finally { setIsLoadingObjects(false); }
  }

  function openBucket(bucket: string) {
    const prefixValue = credentials.prefix.trim().replace(/^\/+|\/+$/g, "");
    setHistory([]);
    void loadLocation({ bucket, prefix: prefixValue ? `${prefixValue}/` : "" });
  }

  function openFolder(entry: S3Entry) {
    if (!location || entry.kind !== "folder") return;
    setHistory((current) => [...current, location]);
    void loadLocation({ bucket: location.bucket, prefix: entry.key });
  }

  function goBack() {
    const previous = history[history.length - 1];
    if (!previous) return;
    setHistory((current) => current.slice(0, -1));
    void loadLocation(previous);
  }

  function goUp() {
    if (!location?.prefix) return;
    const segments = location.prefix.split("/").filter(Boolean);
    segments.pop();
    const parent = segments.length ? `${segments.join("/")}/` : "";
    setHistory((current) => [...current, location]);
    void loadLocation({ bucket: location.bucket, prefix: parent });
  }

  function formatSize(size?: number) {
    if (size === undefined) return "—";
    if (size < 1024) return `${size} B`;
    if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
    if (size < 1024 * 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
    return `${(size / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }

  function formatDate(value?: string) {
    if (!value) return "—";
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }

  function addDownload(item: Omit<DownloadItem, "id" | "status">) {
    setDownloads((current) => [...current, { ...item, id: `${Date.now()}-${Math.random()}`, status: "queued" }]);
    setShowDownloads(true);
  }

  useEffect(() => {
    if (processingDownloads.current) return;
    const next = downloads.find((item) => item.status === "queued");
    if (!next || !location) return;
    processingDownloads.current = true;
    setDownloads((current) => current.map((item) => item.id === next.id ? { ...item, status: "downloading" } : item));
    const command = next.kind === "zip" ? "download_s3_folder_zip" : "download_s3_object";
    const request = next.kind === "zip" ? { bucket: next.bucket, prefix: next.key, destination: next.destination } : { bucket: next.bucket, key: next.key, destination: next.destination };
    void invoke(command, { request }).then(() => setDownloads((current) => current.map((item) => item.id === next.id ? { ...item, status: "completed" } : item))).catch((reason) => setDownloads((current) => current.map((item) => item.id === next.id ? { ...item, status: "failed", error: typeof reason === "string" ? reason : "Download failed." } : item))).finally(() => { processingDownloads.current = false; });
  }, [downloads, location]);

  async function downloadEntry(entry: S3Entry) {
    if (!location || entry.kind !== "file") return;
    setBrowserError("");
    let destination: string | null;
    try {
      destination = await save({ defaultPath: entry.name, title: `Save ${entry.name}` });
    } catch (reason) {
      setBrowserError(typeof reason === "string" ? reason : "Unable to open the save dialog.");
      return;
    }
    if (!destination) return;
    addDownload({ bucket: location.bucket, name: entry.name, key: entry.key, destination, size: entry.size, kind: "file" });
  }

  async function downloadFolder(entry: S3Entry, mode: "zip" | "folder") {
    if (!location || entry.kind !== "folder") return;
    setBrowserError("");
    try {
      if (mode === "zip") {
        const destination = await save({ defaultPath: `${entry.name}.zip`, title: `Save ${entry.name} as ZIP`, filters: [{ name: "ZIP archive", extensions: ["zip"] }] });
        if (!destination) return;
        addDownload({ bucket: location.bucket, name: `${entry.name}.zip`, key: entry.key, destination, kind: "zip" });
      } else {
        const destination = await open({ directory: true, multiple: false, title: `Choose destination for ${entry.name}` });
        if (typeof destination !== "string") return;
        const files = await invoke<FolderFile[]>("list_s3_folder_files", { request: { bucket: location.bucket, prefix: entry.key } });
        files.forEach((file) => addDownload({ bucket: location.bucket, name: file.name, key: file.key, destination: `${destination}\\${file.name}`, size: file.size, kind: "file" }));
      }
    } catch (reason) {
      setBrowserError(typeof reason === "string" ? reason : `Unable to download folder '${entry.name}'.`);
    }
  }

  function renderDownloads() {
    return <div className="downloads-view"><div className="downloads-heading"><div><p className="eyebrow eyebrow--muted">DOWNLOADS</p><h2>Download queue</h2><p>Files are downloaded one at a time, in this order.</p></div><button className="secondary-button" type="button" onClick={() => setShowDownloads(false)}>Back to browser</button></div>{downloads.length === 0 ? <p className="empty-state">No downloads yet.</p> : <div className="download-queue" aria-live="polite">{downloads.map((item, index) => <div className={`download-row download-row--${item.status}`} key={item.id}><span className="download-order">{index + 1}</span><span className="download-file"><strong>{item.name}</strong><small>{item.destination}</small></span><span className="download-status">{item.status === "downloading" && <span className="loading-dot" aria-hidden="true" />}{item.status === "downloading" ? "Downloading…" : item.status === "completed" ? "Completed" : item.status === "failed" ? "Failed" : "Queued"}{item.error && <small>{item.error}</small>}</span></div>)}</div>}</div>;
  }

  if (connection) {
    if (showDownloads) return <main className="app-shell app-shell--connected"><section className="connected-panel"><button className="downloads-button" type="button" onClick={() => setShowDownloads(false)}>Back to browser</button>{renderDownloads()}</section></main>;
    return <main className="app-shell app-shell--connected"><section className="connected-panel"><button className="downloads-button" type="button" onClick={() => setShowDownloads(true)}>Downloads <span>{downloads.filter((item) => item.status === "queued" || item.status === "downloading").length}</span></button>
      {!location ? <><div className="connected-header"><div><p className="eyebrow eyebrow--muted">CONNECTED</p><h2>Your Amazon S3 buckets</h2><p>Connection verified in {connection.region}.</p></div><button className="secondary-button" type="button" onClick={() => setConnection(null)}>Edit connection</button></div>
      <div className="bucket-list" aria-live="polite">{connection.buckets.length === 0 ? <p className="empty-state">No buckets were returned for this account.</p> : connection.buckets.map((bucket) => <button className="bucket-row" key={bucket.name} type="button" onClick={() => openBucket(bucket.name)}><span className="bucket-icon" aria-hidden="true">▱</span><span>{bucket.name}</span><span className="row-chevron" aria-hidden="true">›</span></button>)}</div>
      <p className="permissions-note">Select a bucket to browse its folders and files.</p></> : <>
        <div className="browser-toolbar"><div className="browser-actions"><button className="icon-button" type="button" onClick={goBack} disabled={!history.length} aria-label="Back" title="Back">←</button><button className="icon-button" type="button" onClick={goUp} disabled={!location.prefix} aria-label="Go to upper directory" title="Go to upper directory">↑</button></div><button className="secondary-button" type="button" onClick={() => { setLocation(null); setHistory([]); }}>All buckets</button></div>
        <div className="browser-heading"><p className="eyebrow eyebrow--muted">BUCKET</p><h2>{location.bucket}</h2><p className="breadcrumb">s3://{location.bucket}/{location.prefix}</p></div>
        {browserError && <p className="form-message form-message--error">{browserError}</p>}
        <div className="object-table" aria-live="polite"><div className="object-row object-row--header"><span>Name</span><span>Type</span><span>Size</span><span>Modified</span><span>Actions</span></div>{isLoadingObjects ? <p className="empty-state">Loading objects…</p> : entries.length === 0 ? <p className="empty-state">This directory is empty.</p> : entries.map((entry) => <button className={`object-row ${selectedKey === entry.key ? "object-row--selected" : ""}`} key={entry.key} type="button" onClick={() => entry.kind === "folder" ? openFolder(entry) : setSelectedKey(entry.key)}><span className="object-name"><span className="object-icon" aria-hidden="true">{entry.kind === "folder" ? "▰" : "□"}</span>{entry.name}</span><span>{entry.kind === "folder" ? "Folder" : "File"}</span><span>{formatSize(entry.size)}</span><span>{formatDate(entry.lastModified)}</span><span>{entry.kind === "folder" ? <span className="download-actions"><button className="download-button" type="button" disabled={Boolean(downloadingKey || downloadingFolderKey)} onClick={(event) => { event.stopPropagation(); void downloadFolder(entry, "zip"); }}>{downloadingFolderKey === entry.key ? "Downloading…" : "Download ZIP"}</button><button className="download-button" type="button" disabled={Boolean(downloadingKey || downloadingFolderKey)} onClick={(event) => { event.stopPropagation(); void downloadFolder(entry, "folder"); }}>Extract</button></span> : <button className="download-button" type="button" disabled={Boolean(downloadingKey || downloadingFolderKey)} onClick={(event) => { event.stopPropagation(); void downloadEntry(entry); }}>{downloadingKey === entry.key ? "Downloading…" : "Download"}</button>}</span></button>)}</div>
      </>}
    </section></main>;
  }

  return <main className="app-shell"><section className="brand-panel" aria-label="S3 Browser introduction"><div className="brand-mark" aria-hidden="true"><span className="brand-mark__top" /><span className="brand-mark__bottom" /></div><div className="brand-copy"><p className="eyebrow">AWS S3 BROWSER</p><h1>Everything in your buckets, one quiet workspace.</h1><p className="brand-description">Connect securely to Amazon S3 and browse, download, and organize your objects from one focused desktop app.</p></div><div className="brand-footer"><span className="status-dot" /><span>Local-first workspace</span></div></section>
    <section className="login-panel"><div className="login-content"><div className="mobile-brand" aria-hidden="true"><div className="brand-mark brand-mark--small"><span className="brand-mark__top" /><span className="brand-mark__bottom" /></div><span className="mobile-brand__name">S3 Browser</span></div><div className="login-heading"><p className="eyebrow eyebrow--muted">WELCOME BACK</p><h2>Connect to Amazon S3</h2><p>Use an IAM access key with the minimum permissions you need.</p></div>
      <form className="credentials-form" onSubmit={handleSubmit}><div className="field-group"><label htmlFor="access-key">Access key ID</label><input id="access-key" type="text" autoComplete="username" placeholder="AKIA..." value={credentials.accessKeyId} onChange={(e) => updateField("accessKeyId", e.target.value)} /></div><div className="field-group"><div className="label-row"><label htmlFor="secret-key">Secret access key</label><span className="secure-label">🔒 Encrypted in Windows Credential Manager</span></div><div className="input-with-action"><input id="secret-key" type={showSecret ? "text" : "password"} autoComplete="current-password" placeholder="Enter your secret key" value={credentials.secretAccessKey} onChange={(e) => updateField("secretAccessKey", e.target.value)} /><button className="input-action" type="button" onClick={() => setShowSecret((current) => !current)}>{showSecret ? "Hide" : "Show"}</button></div></div><div className="form-row"><div className="field-group field-group--region"><label htmlFor="region">AWS region</label><select id="region" value={credentials.region} onChange={(e) => updateField("region", e.target.value)}>{regions.map(([value, label]) => <option key={value} value={value}>{label} ({value})</option>)}</select></div><div className="field-group"><label htmlFor="bucket">Bucket <span className="optional">optional</span></label><input id="bucket" type="text" placeholder="my-bucket" value={credentials.bucket} onChange={(e) => updateField("bucket", e.target.value)} /></div></div><div className="field-group"><label htmlFor="prefix">Starting folder <span className="optional">optional</span></label><div className="prefix-input"><span aria-hidden="true">s3://</span><input id="prefix" type="text" placeholder="folder/" value={credentials.prefix} onChange={(e) => updateField("prefix", e.target.value)} /></div></div>{error && <p className="form-message form-message--error">{error}</p>}<button className="connect-button" type="submit" disabled={isConnecting}><span>{isConnecting ? "Connecting…" : "Connect to S3"}</span><span className="arrow" aria-hidden="true">→</span></button></form><p className="permissions-note"><span className="info-icon" aria-hidden="true">i</span> We recommend an IAM policy limited to the bucket you want to access.</p></div><p className="version-label">S3 Browser <span>·</span> Desktop</p></section></main>;
}

export default App;
