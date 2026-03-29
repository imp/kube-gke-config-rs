//! # kube-gke-config
//!
//! Helpers for building a [`kube::Client`] (or [`kube::Config`])
//! directly from a [Google Kubernetes Engine](https://cloud.google.com/kubernetes-engine)
//! cluster, without manually managing a kubeconfig file on disk.
//!
//! ## How it works
//!
//! The crate calls the GKE `GetCluster` API to retrieve the cluster's HTTPS
//! endpoint IP and certificate-authority data, then converts those values into
//! the configuration structs used by `kube_client`.  Authentication is
//! intentionally omitted from the static config: GKE uses short-lived OAuth2
//! tokens obtained via Workload Identity or Application Default Credentials
//! (ADC) — none of which belong in a static config.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use kube_gke_config::{TryGkeClusterExt, default_gke_client};
//!
//! #[tokio::main]
//! async fn main() -> kube::Result<()> {
//!     // Credentials are loaded from the environment (see [`default_gke_client`])
//!     let gke = default_gke_client().await
//!         .map_err(|err| kube::Error::Service(Box::new(err)))?;
//!
//!     // One call produces a ready-to-use Kubernetes client
//!     let client = gke
//!         .try_gke_kube_client("my-project", "us-central1", "my-cluster")
//!         .await?;
//!     let _ = client;
//!     Ok(())
//! }
//! ```
//!
//! ## GKE credentials
//!
//! [`default_gke_client`] resolves credentials via the standard Google ADC
//! chain (highest priority first):
//!
//! 1. **`GOOGLE_APPLICATION_CREDENTIALS`** — path to a service account JSON
//!    key file.
//! 2. **User credentials** — set by `gcloud auth application-default login`.
//! 3. **Metadata server** — service account attached to the running Google
//!    Cloud resource (Compute Engine, GKE Workload Identity, Cloud Run, etc.).
//!
//! Any custom [`google_cloud_container_v1::client::ClusterManager`] can also
//! be used directly with the [`TryGkeClusterExt`] methods.
//!
//! ## Traits at a glance
//!
//! | Trait | Input | Output |
//! |---|---|---|
//! | [`TryGkeClusterExt`] | `ClusterManager` + project/location/cluster | cluster / config / client |
//! | [`ToKubeConfig`] | `gke::model::Cluster` | `kube::Config` |
//! | [`IntoKubeconfig`] | `gke::model::Cluster` | `kube::config::Kubeconfig` |

use google_cloud_container_v1 as gke;
use google_cloud_gax as gax;
use kube_client::config as kubeconfig;

/// Extension trait that adds GKE-aware helpers to
/// [`google_cloud_container_v1::client::ClusterManager`].
///
/// The three methods form a convenience ladder — use the one that returns
/// exactly what you need without paying for extra GKE API calls:
///
/// | Method | Returns |
/// |---|---|
/// | [`try_gke_cluster`](Self::try_gke_cluster) | Raw [`gke::model::Cluster`] from the GKE API |
/// | [`try_gke_kube_config`](Self::try_gke_kube_config) | [`kube::Config`] ready for `Client::try_from` |
/// | [`try_gke_kube_client`](Self::try_gke_kube_client) | Fully initialised [`kube::Client`] |
///
/// Most callers only need [`try_gke_kube_client`](Self::try_gke_kube_client).
/// The lower-level methods are exposed so that intermediate values can be
/// inspected or reused without making additional GKE API calls.
///
/// # Example
///
/// ```rust,no_run
/// use kube_gke_config::{TryGkeClusterExt, default_gke_client};
///
/// # #[tokio::main]
/// # async fn main() -> kube::Result<()> {
/// let gke = default_gke_client().await
///     .map_err(|err| kube::Error::Service(Box::new(err)))?;
/// let client = gke
///     .try_gke_kube_client("my-project", "us-central1", "my-cluster")
///     .await?;
/// let _ = client;
/// # Ok(())
/// # }
/// ```
// `async fn` in traits is stable since Rust 1.75 but still triggers a lint;
// `#[expect]` silences it and documents the intent.
#[expect(async_fn_in_trait)]
pub trait TryGkeClusterExt {
    /// Fetches the GKE cluster descriptor from the API.
    ///
    /// Returns the raw [`gke::model::Cluster`] struct, which contains the
    /// HTTPS endpoint IP, certificate-authority data, cluster status,
    /// Kubernetes version, and other metadata.
    ///
    /// The cluster is identified by the three-part resource path
    /// `projects/{project}/locations/{location}/clusters/{name}`.
    ///
    /// # Errors
    ///
    /// Returns a [`gke::Error`] on API or network failures, including when no
    /// cluster with the given project/location/name exists.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use kube_gke_config::{TryGkeClusterExt, default_gke_client};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    /// let gke = default_gke_client().await?;
    /// let cluster = gke
    ///     .try_gke_cluster("my-project", "us-central1", "my-cluster")
    ///     .await?;
    /// println!("Kubernetes version: {}", cluster.current_master_version);
    /// # Ok(())
    /// # }
    /// ```
    async fn try_gke_cluster(
        &self,
        project: impl Into<String>,
        location: impl Into<String>,
        cluster: impl Into<String>,
    ) -> Result<gke::model::Cluster, gke::Error>;

    /// Builds a [`kube::Config`] for the named GKE cluster.
    ///
    /// This is a provided method: it calls [`try_gke_cluster`](Self::try_gke_cluster)
    /// and converts the result via [`ToKubeConfig::into_kube_config`].
    ///
    /// The resulting `Config` holds the cluster's HTTPS endpoint and
    /// certificate-authority data but does **not** contain authentication
    /// credentials — GKE authentication is handled via short-lived OAuth2
    /// tokens (Workload Identity, ADC).
    ///
    /// # Errors
    ///
    /// - [`kube::Error::Service`] wrapping a [`gke::Error`] on GKE API
    ///   failures.
    /// - [`kube::Error::InferKubeconfig`] if the endpoint is absent or
    ///   cannot be parsed as a URL.
    async fn try_gke_kube_config(
        &self,
        project: impl Into<String>,
        location: impl Into<String>,
        cluster: impl Into<String>,
    ) -> kube_client::Result<kube_client::Config> {
        self.try_gke_cluster(project, location, cluster)
            .await
            .map_err(|err| kube_client::Error::Service(Box::new(err)))?
            .into_kube_config()
            .map_err(kube_client::Error::InferKubeconfig)
    }

    /// Creates a [`kube::Client`] connected to the named GKE cluster.
    ///
    /// This is the primary convenience method. It combines
    /// [`try_gke_kube_config`](Self::try_gke_kube_config) and
    /// [`kube::Client::try_from`] into a single call.
    ///
    /// # Errors
    ///
    /// Propagates all errors from
    /// [`try_gke_kube_config`](Self::try_gke_kube_config) plus any TLS or HTTP
    /// client initialisation errors from `kube_client`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use kube_gke_config::{TryGkeClusterExt, default_gke_client};
    ///
    /// #[tokio::main]
    /// async fn main() -> kube::Result<()> {
    ///     let gke = default_gke_client().await
    ///         .map_err(|err| kube::Error::Service(Box::new(err)))?;
    ///     let client = gke
    ///         .try_gke_kube_client("my-project", "us-central1", "my-cluster")
    ///         .await?;
    ///     let _ = client; // use with kube::Api for Kubernetes operations
    ///     Ok(())
    /// }
    /// ```
    async fn try_gke_kube_client(
        &self,
        project: impl Into<String>,
        location: impl Into<String>,
        cluster: impl Into<String>,
    ) -> kube_client::Result<kube_client::Client> {
        let config = self.try_gke_kube_config(project, location, cluster).await?;
        kube_client::Client::try_from(config)
    }
}

impl TryGkeClusterExt for gke::client::ClusterManager {
    async fn try_gke_cluster(
        &self,
        project: impl Into<String>,
        location: impl Into<String>,
        cluster: impl Into<String>,
    ) -> Result<gke::model::Cluster, gke::Error> {
        let name = format!(
            "projects/{}/locations/{}/clusters/{}",
            project.into(),
            location.into(),
            cluster.into(),
        );
        self.get_cluster().set_name(name).send().await
    }
}

/// Converts a [`gke::model::Cluster`] into a [`kube::Config`].
///
/// This lower-level conversion is used internally by
/// [`TryGkeClusterExt::try_gke_kube_config`]. It is also useful when you
/// already hold a `Cluster` value and want a runtime `Config` without making
/// an additional GKE API call.
///
/// See also [`IntoKubeconfig`] for converting to the serialisable
/// [`kube::config::Kubeconfig`] format (the equivalent of a kubeconfig
/// YAML file).
pub trait ToKubeConfig {
    /// Converts `self` into a [`kube::Config`].
    ///
    /// Extracts the cluster's `endpoint` (required, must be non-empty) and
    /// `master_auth.cluster_ca_certificate` (optional). No authentication
    /// credentials are included — GKE uses short-lived OAuth2 tokens.
    ///
    /// The `endpoint` field on GKE clusters contains a bare IP address; this
    /// method prepends `https://` automatically.
    ///
    /// # Errors
    ///
    /// - [`kube::config::KubeconfigError::MissingClusterUrl`] if the
    ///   cluster's `endpoint` field is empty.
    /// - [`kube::config::KubeconfigError::ParseClusterUrl`] if the
    ///   endpoint cannot be parsed as a valid URL.
    fn into_kube_config(self) -> Result<kubeconfig::Config, kubeconfig::KubeconfigError>;
}

impl ToKubeConfig for gke::model::Cluster {
    fn into_kube_config(self) -> Result<kubeconfig::Config, kubeconfig::KubeconfigError> {
        use base64::Engine as _;

        // GKE returns cluster_ca_certificate as a base64-encoded PEM string.
        // Decode it to raw PEM bytes once; reuse for both root_cert and exec.cluster.
        let ca_pem_bytes = self
            .master_auth
            .as_ref()
            .filter(|auth| !auth.cluster_ca_certificate.is_empty())
            .and_then(|auth| {
                base64::engine::general_purpose::STANDARD
                    .decode(&auth.cluster_ca_certificate)
                    .ok()
            });

        // root_cert: DER bytes used by rustls as the TLS trust anchor.
        let root_cert = ca_pem_bytes
            .as_deref()
            .and_then(|pem_bytes| pem::parse(pem_bytes).ok())
            .map(|p| vec![p.into_contents()]);

        // GKE endpoint is a bare IP address; prepend the HTTPS scheme.
        let endpoint_url = self
            .endpoint()
            .ok_or(kubeconfig::KubeconfigError::MissingClusterUrl)?;
        let cluster_url = endpoint_url
            .parse()
            .map_err(kubeconfig::KubeconfigError::ParseClusterUrl)?;

        // exec.cluster must be set when provide_cluster_info = true so that
        // kube-client can pass cluster info to gke-gcloud-auth-plugin via
        // KUBERNETES_EXEC_INFO.  When loading from a Kubeconfig file, the
        // ConfigLoader fills this in automatically; building Config directly
        // means we have to do it ourselves.
        let exec_cluster = kubeconfig::ExecAuthCluster {
            server: Some(endpoint_url),
            certificate_authority_data: ca_pem_bytes,
            ..kubeconfig::ExecAuthCluster::default()
        };
        let exec = kubeconfig::ExecConfig {
            api_version: Some("client.authentication.k8s.io/v1beta1".to_string()),
            command: Some("gke-gcloud-auth-plugin".to_string()),
            args: None,
            env: None,
            drop_env: None,
            interactive_mode: None,
            provide_cluster_info: true,
            cluster: Some(exec_cluster),
        };
        let config = kubeconfig::Config {
            root_cert,
            auth_info: kubeconfig::AuthInfo {
                exec: Some(exec),
                ..kubeconfig::AuthInfo::default()
            },
            ..kubeconfig::Config::new(cluster_url)
        };
        Ok(config)
    }
}

/// Converts a [`gke::model::Cluster`] into a
/// [`kube::config::Kubeconfig`].
///
/// Unlike [`ToKubeConfig`] (which produces a `kube::Config` runtime
/// struct), this trait produces the serialisable
/// [`kube::config::Kubeconfig`] structure — the in-memory equivalent of
/// a `~/.kube/config` file — with named cluster, context, and
/// `current-context` entries.
///
/// This is useful when you need to:
///
/// - Serialise the kubeconfig to YAML and write it to disk.
/// - Merge the GKE cluster entry into an existing kubeconfig.
/// - Pass a structured kubeconfig to tooling that expects the full format.
///
/// # Authentication
///
/// The produced `Kubeconfig` includes an `auth_infos` entry that uses the
/// `gke-gcloud-auth-plugin` exec plugin (the same mechanism as `gcloud
/// container clusters get-credentials`). The plugin obtains short-lived OAuth2
/// tokens at runtime via Application Default Credentials.
///
/// See also [`ToKubeConfig`] for a direct runtime `Config` conversion.
pub trait IntoKubeconfig {
    /// Converts `self` into a [`kube::config::Kubeconfig`].
    ///
    /// The cluster name (falls back to `"gke-cluster"` if empty) is used as
    /// the `clusters[0].name`, `contexts[0].name`,
    /// `contexts[0].context.cluster`, and `current-context`.
    ///
    /// # Errors
    ///
    /// Currently infallible in practice, but returns a `Result` for forward
    /// compatibility.
    fn into_kubeconfig(self) -> Result<kubeconfig::Kubeconfig, kubeconfig::KubeconfigError>;
}

impl IntoKubeconfig for gke::model::Cluster {
    fn into_kubeconfig(self) -> Result<kubeconfig::Kubeconfig, kubeconfig::KubeconfigError> {
        let name = self.name();
        let server = self.endpoint();
        let certificate_authority_data = self
            .master_auth
            .map(|auth| auth.cluster_ca_certificate)
            .filter(|s| !s.is_empty());
        let cluster = kubeconfig::Cluster {
            server,
            certificate_authority_data,
            ..kubeconfig::Cluster::default()
        };

        let named_cluster = kubeconfig::NamedCluster {
            name: name.clone(),
            cluster: Some(cluster),
        };

        let exec = kubeconfig::ExecConfig {
            api_version: Some("client.authentication.k8s.io/v1beta1".to_string()),
            command: Some("gke-gcloud-auth-plugin".to_string()),
            args: None,
            env: None,
            drop_env: None,
            interactive_mode: None,
            provide_cluster_info: true,
            cluster: None,
        };
        let named_auth_info = kubeconfig::NamedAuthInfo {
            name: name.clone(),
            auth_info: Some(kubeconfig::AuthInfo {
                exec: Some(exec),
                ..kubeconfig::AuthInfo::default()
            }),
        };

        let context = kubeconfig::Context {
            cluster: name.clone(),
            user: Some(name.clone()),
            ..kubeconfig::Context::default()
        };

        let named_context = kubeconfig::NamedContext {
            name: name.clone(),
            context: Some(context),
        };

        let config = kubeconfig::Kubeconfig {
            clusters: vec![named_cluster],
            auth_infos: vec![named_auth_info],
            contexts: vec![named_context],
            current_context: Some(name),
            ..kubeconfig::Kubeconfig::default()
        };

        Ok(config)
    }
}

/// Creates a [`google_cloud_container_v1::client::ClusterManager`] using
/// Application Default Credentials (ADC).
///
/// Credentials are resolved in the following order (highest priority first):
///
/// 1. **`GOOGLE_APPLICATION_CREDENTIALS`** — path to a service account JSON
///    key file.
/// 2. **User credentials** — set by `gcloud auth application-default login`.
/// 3. **Metadata server** — service account attached to the running Google
///    Cloud resource (Compute Engine, GKE Workload Identity, Cloud Run, etc.).
///
/// This is a thin convenience wrapper around
/// [`google_cloud_container_v1::client::ClusterManager::builder`].
/// For fine-grained control over credentials or the API endpoint, build a
/// `ClusterManager` directly and use [`TryGkeClusterExt`] on it.
///
/// # Errors
///
/// Returns a boxed error if credentials cannot be resolved or the HTTP
/// client cannot be initialised.
///
/// # Example
///
/// ```rust,no_run
/// use kube_gke_config::{TryGkeClusterExt, default_gke_client};
///
/// # #[tokio::main]
/// # async fn main() -> kube::Result<()> {
/// let gke = default_gke_client().await
///     .map_err(|err| kube::Error::Service(Box::new(err)))?;
/// let client = gke
///     .try_gke_kube_client("my-project", "us-central1", "my-cluster")
///     .await?;
/// let _ = client;
/// # Ok(())
/// # }
/// ```
pub async fn default_gke_client() -> gax::client_builder::Result<gke::client::ClusterManager> {
    gke::client::ClusterManager::builder().build().await
}

trait ClusterExt {
    fn name(&self) -> String;
    fn endpoint(&self) -> Option<String>;
}

impl ClusterExt for gke::model::Cluster {
    fn name(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else {
            "gke-cluster".to_string()
        }
    }

    fn endpoint(&self) -> Option<String> {
        if !self.endpoint.is_empty() {
            Some(format!("https://{}", self.endpoint))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use google_cloud_container_v1 as gke;
    use kube_client::config as kubeconfig;

    use super::*;

    /// Constructs a [`gke::model::Cluster`] from parts without hitting the GKE API.
    fn make_cluster(
        name: Option<&str>,
        endpoint: Option<&str>,
        ca_cert: Option<&str>,
    ) -> gke::model::Cluster {
        let mut cluster = gke::model::Cluster::default();
        if let Some(n) = name {
            cluster = cluster.set_name(n);
        }
        if let Some(e) = endpoint {
            cluster = cluster.set_endpoint(e);
        }
        if let Some(c) = ca_cert {
            let auth = gke::model::MasterAuth::new().set_cluster_ca_certificate(c);
            cluster = cluster.set_or_clear_master_auth(Some(auth));
        }
        cluster
    }

    // A real self-signed EC cert (base64-encoded PEM) used as test CA data.
    const TEST_CA_CERT_B64: &str = "LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0tCk1JSUNCRENDQWFvQ0NRQ05JNzRXSFNXMXVEQUtCZ2dxaGtqT1BRUURBakFQTVEwd0N3WURWUVFEREFSMFpYTjAKTUNBWERUSTJNRE15T1RFek5UY3hObG9ZRHpJeE1qWXdNekExTVRNMU56RTJXakFQTVEwd0N3WURWUVFEREFSMApaWE4wTUlJQlN6Q0NBUU1HQnlxR1NNNDlBZ0V3Z2ZjQ0FRRXdMQVlIS29aSXpqMEJBUUloQVAvLy8vOEFBQUFCCkFBQUFBQUFBQUFBQUFBQUEvLy8vLy8vLy8vLy8vLy8vTUZzRUlQLy8vLzhBQUFBQkFBQUFBQUFBQUFBQUFBQUEKLy8vLy8vLy8vLy8vLy8vOEJDQmF4alhZcWpxVDU3UHJ2VlYybUlhOFpSMEdzTXhUc1BZN3pqdytKOUpnU3dNVgpBTVNkTmdpRzV3U1RhbVo0NFJPZEpyZUJuMzZRQkVFRWF4ZlI4dUVzUWtmNHZPYmxZNlJBOG5jRGZZRXQ2ek9nCjlLRTVSZGlZd3BaUDQwTGkvaHAvbTQ3bjYwcDhENTRXSzg0elYyc3hYczdMdGtCb043OVI5UUloQVAvLy8vOEEKQUFBQS8vLy8vLy8vLy8rODV2cXRweGVlaFBPNXlzTDhZeVZSQWdFQkEwSUFCQldxNVVpaVczeElyQVpGNkx1UAo3SmZVd1REWWZUS0pBN2lDbGxXb0ZUQmxtYjFTL3RuVUd6aGF2eTA3eFhUeGFhcUh0MUVwU3RoaFFwQ3Z0RHVxCkkrd3dDZ1lJS29aSXpqMEVBd0lEU0FBd1JRSWhBS3RORGNxZGc3emZXcDdjTmJDZ1VkbUxra3UzMHh1QmR3aUMKMEhpTHBPbGhBaUJEeXJuc1BZUFhXemNYY21ieVdUNEJLSXcraVJIM0g2OGFNYml6U1MwQnRnPT0KLS0tLS1FTkQgQ0VSVElGSUNBVEUtLS0tLQo=";

    #[test]
    fn to_kube_config_has_gke_exec_auth() {
        let cluster = make_cluster(Some("test"), Some("35.200.100.50"), None);
        let config = cluster.into_kube_config().expect("should build config");
        let exec = config.auth_info.exec.as_ref().expect("exec should be set");
        assert_eq!(exec.command.as_deref(), Some("gke-gcloud-auth-plugin"));
        assert!(exec.provide_cluster_info);
    }

    #[test]
    fn to_kube_config_extracts_endpoint_and_cert() {
        let cluster = make_cluster(Some("test"), Some("35.200.100.50"), Some(TEST_CA_CERT_B64));
        let config = cluster.into_kube_config().expect("should build config");
        assert_eq!(config.cluster_url.host(), Some("35.200.100.50"));
        assert!(
            config
                .root_cert
                .as_ref()
                .is_some_and(|certs| !certs.is_empty()),
            "expected root_cert to be populated from CA cert"
        );
    }

    #[test]
    fn to_kube_config_empty_endpoint_returns_error() {
        let cluster = make_cluster(Some("test"), None, None);
        let err = cluster.into_kube_config().unwrap_err();
        assert!(
            matches!(err, kubeconfig::KubeconfigError::MissingClusterUrl),
            "expected MissingClusterUrl, got {err:?}"
        );
    }

    #[test]
    fn to_kube_config_no_master_auth_has_no_cert() {
        let cluster = make_cluster(Some("test"), Some("35.200.100.50"), None);
        let config = cluster.into_kube_config().expect("should build config");
        assert!(
            config.root_cert.is_none(),
            "expected no root_cert when master_auth is absent"
        );
    }

    #[test]
    fn into_kubeconfig_uses_cluster_name() {
        let cluster = make_cluster(Some("my-cluster"), Some("35.200.100.50"), None);
        let kc = cluster.into_kubeconfig().expect("should build kubeconfig");
        assert_eq!(kc.current_context.as_deref(), Some("my-cluster"));
        assert_eq!(kc.clusters[0].name, "my-cluster");
        assert_eq!(kc.contexts[0].name, "my-cluster");
        assert_eq!(
            kc.contexts[0].context.as_ref().map(|c| c.cluster.as_str()),
            Some("my-cluster")
        );
        // context.user must match the auth_infos entry
        assert_eq!(
            kc.contexts[0]
                .context
                .as_ref()
                .and_then(|c| c.user.as_deref()),
            Some("my-cluster")
        );
        assert_eq!(kc.auth_infos[0].name, "my-cluster");
        let exec = kc.auth_infos[0]
            .auth_info
            .as_ref()
            .and_then(|a| a.exec.as_ref())
            .expect("exec should be set");
        assert_eq!(exec.command.as_deref(), Some("gke-gcloud-auth-plugin"));
        assert!(exec.provide_cluster_info);
    }

    #[test]
    fn into_kubeconfig_falls_back_to_gke_cluster_name() {
        let cluster = make_cluster(None, Some("35.200.100.50"), None);
        let kc = cluster.into_kubeconfig().expect("should build kubeconfig");
        assert_eq!(kc.current_context.as_deref(), Some("gke-cluster"));
        assert_eq!(kc.clusters[0].name, "gke-cluster");
    }

    #[test]
    fn into_kubeconfig_propagates_cert_authority_data() {
        let cluster = make_cluster(Some("test"), None, Some("dGVzdA=="));
        let kc = cluster.into_kubeconfig().expect("should build kubeconfig");
        let cert = kc.clusters[0]
            .cluster
            .as_ref()
            .and_then(|c| c.certificate_authority_data.as_deref());
        assert_eq!(cert, Some("dGVzdA=="));
    }
}
