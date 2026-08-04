import { FormEvent, useState } from "react";
import "./App.css";

type S3Credentials = {
  accessKeyId: string;
  secretAccessKey: string;
  region: string;
  bucket: string;
  prefix: string;
};

const regions = [
  { value: "us-east-1", label: "US East (N. Virginia)" },
  { value: "us-west-2", label: "US West (Oregon)" },
  { value: "eu-central-1", label: "Europe (Frankfurt)" },
  { value: "eu-west-1", label: "Europe (Ireland)" },
  { value: "ap-southeast-1", label: "Asia Pacific (Singapore)" },
];

function App() {
  const [credentials, setCredentials] = useState<S3Credentials>({
    accessKeyId: "",
    secretAccessKey: "",
    region: "eu-central-1",
    bucket: "",
    prefix: "",
  });
  const [showSecret, setShowSecret] = useState(false);
  const [error, setError] = useState("");
  const [isReady, setIsReady] = useState(false);

  function updateField(field: keyof S3Credentials, value: string) {
    setCredentials((current) => ({ ...current, [field]: value }));
    setError("");
    setIsReady(false);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    if (!credentials.accessKeyId.trim() || !credentials.secretAccessKey.trim()) {
      setError("Enter both your access key ID and secret access key to continue.");
      return;
    }

    setIsReady(true);
  }

  return (
    <main className="app-shell">
      <section className="brand-panel" aria-label="S3 Browser introduction">
        <div className="brand-mark" aria-hidden="true">
          <span className="brand-mark__top" />
          <span className="brand-mark__bottom" />
        </div>
        <div className="brand-copy">
          <p className="eyebrow">AWS S3 BROWSER</p>
          <h1>Everything in your buckets, one quiet workspace.</h1>
          <p className="brand-description">
            Connect securely to Amazon S3 and browse, download, and organize
            your objects from one focused desktop app.
          </p>
        </div>
        <div className="brand-footer">
          <span className="status-dot" />
          <span>Local-first workspace</span>
        </div>
      </section>

      <section className="login-panel">
        <div className="login-content">
          <div className="mobile-brand" aria-hidden="true">
            <div className="brand-mark brand-mark--small">
              <span className="brand-mark__top" />
              <span className="brand-mark__bottom" />
            </div>
            <span className="mobile-brand__name">S3 Browser</span>
          </div>

          <div className="login-heading">
            <p className="eyebrow eyebrow--muted">WELCOME BACK</p>
            <h2>Connect to Amazon S3</h2>
            <p>Use an IAM access key with the minimum permissions you need.</p>
          </div>

          <form className="credentials-form" onSubmit={handleSubmit}>
            <div className="field-group">
              <label htmlFor="access-key">Access key ID</label>
              <input
                id="access-key"
                type="text"
                autoComplete="username"
                placeholder="AKIA..."
                value={credentials.accessKeyId}
                onChange={(event) => updateField("accessKeyId", event.target.value)}
              />
            </div>

            <div className="field-group">
              <div className="label-row">
                <label htmlFor="secret-key">Secret access key</label>
                <span className="secure-label">
                  <span className="lock-icon" aria-hidden="true">⌑</span>
                  Encrypted in memory
                </span>
              </div>
              <div className="input-with-action">
                <input
                  id="secret-key"
                  type={showSecret ? "text" : "password"}
                  autoComplete="current-password"
                  placeholder="Enter your secret key"
                  value={credentials.secretAccessKey}
                  onChange={(event) => updateField("secretAccessKey", event.target.value)}
                />
                <button
                  className="input-action"
                  type="button"
                  onClick={() => setShowSecret((current) => !current)}
                  aria-label={showSecret ? "Hide secret access key" : "Show secret access key"}
                >
                  {showSecret ? "Hide" : "Show"}
                </button>
              </div>
            </div>

            <div className="form-row">
              <div className="field-group field-group--region">
                <label htmlFor="region">AWS region</label>
                <select
                  id="region"
                  value={credentials.region}
                  onChange={(event) => updateField("region", event.target.value)}
                >
                  {regions.map((region) => (
                    <option key={region.value} value={region.value}>
                      {region.label} ({region.value})
                    </option>
                  ))}
                </select>
              </div>
              <div className="field-group">
                <label htmlFor="bucket">
                  Bucket <span className="optional">optional</span>
                </label>
                <input
                  id="bucket"
                  type="text"
                  placeholder="my-bucket"
                  value={credentials.bucket}
                  onChange={(event) => updateField("bucket", event.target.value)}
                />
              </div>
            </div>

            <div className="field-group">
              <label htmlFor="prefix">
                Starting folder <span className="optional">optional</span>
              </label>
              <div className="prefix-input">
                <span aria-hidden="true">s3://</span>
                <input
                  id="prefix"
                  type="text"
                  placeholder="folder/"
                  value={credentials.prefix}
                  onChange={(event) => updateField("prefix", event.target.value)}
                />
              </div>
            </div>

            {error && <p className="form-message form-message--error">{error}</p>}
            {isReady && (
              <p className="form-message form-message--success">
                Connection details are ready for {credentials.region}. Your secret key remains in memory only.
              </p>
            )}

            <button className="connect-button" type="submit">
              <span>{isReady ? "Connection ready" : "Connect to S3"}</span>
              <span className="arrow" aria-hidden="true">→</span>
            </button>
          </form>

          <p className="permissions-note">
            <span className="info-icon" aria-hidden="true">i</span>
            We recommend an IAM policy limited to the bucket you want to access.
          </p>
        </div>
        <p className="version-label">S3 Browser <span>·</span> Desktop</p>
      </section>
    </main>
  );
}

export default App;
