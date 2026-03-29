# kube-gke-config

[![Crates.io](https://img.shields.io/crates/v/kube-gke-config)](https://crates.io/crates/kube-gke-config)
[![Docs.rs](https://docs.rs/kube-gke-config/badge.svg)](https://docs.rs/kube-gke-config)
[![License](https://img.shields.io/crates/l/kube-gke-config)](LICENSE)

Build a [`kube::Client`](https://docs.rs/kube/latest/kube/struct.Client.html) (or
[`kube::Config`](https://docs.rs/kube/latest/kube/struct.Config.html)) directly from a
[Google Kubernetes Engine](https://cloud.google.com/kubernetes-engine) cluster, without
managing a kubeconfig file on disk.

## How it works

1. Calls the GKE `GetCluster` API via
   [`google-cloud-container-v1`](https://crates.io/crates/google-cloud-container-v1) to
   retrieve the cluster's HTTPS endpoint IP and certificate-authority data.
2. Constructs a `kube::Config` from those values.
3. Returns a `kube::Client` ready to use.

Authentication tokens are **not** included in the config. GKE uses short-lived OAuth2
tokens obtained at runtime via Workload Identity or Application Default Credentials (ADC).

## Quick start

```rust,no_run
use kube_gke_config::{TryGkeClusterExt, default_gke_client};

#[tokio::main]
async fn main() -> kube::Result<()> {
    let gke = default_gke_client().await
        .map_err(kube::Error::Service)?;

    let client = gke
        .try_gke_kube_client("my-project", "us-central1", "my-cluster")
        .await?;

    // Use client with kube::Api<Pod>, kube::Api<Deployment>, …
    let _ = client;
    Ok(())
}
```

## Step-by-step

Each method on `TryGkeClusterExt` returns more than the previous one, so you can stop
at whichever level you need:

```rust,no_run
use kube_gke_config::{TryGkeClusterExt, default_gke_client};
use kube::ResourceExt;
use kube::api;
use k8s_openapi::api::core::v1 as corev1;

#[tokio::main]
async fn main() -> kube::Result<()> {
    let gke = default_gke_client().await
        .map_err(kube::Error::Service)?;

    // Level 1 – raw GKE cluster struct
    let info = gke
        .try_gke_cluster("my-project", "us-central1", "my-cluster")
        .await
        .map_err(|e| kube::Error::Service(Box::new(e)))?;
    println!("endpoint: {}", info.endpoint);
    println!("k8s version: {}", info.current_master_version);

    // Level 2 – kube::Config
    let config = gke
        .try_gke_kube_config("my-project", "us-central1", "my-cluster")
        .await?;
    println!("URL: {}", config.cluster_url);

    // Level 3 – kube::Client
    let client = kube::Client::try_from(config)?;
    let lp = api::ListParams::default();
    let pods = api::Api::<corev1::Pod>::default_namespaced(client)
        .list(&lp)
        .await?;
    for pod in pods {
        println!("{}", pod.name_any());
    }
    Ok(())
}
```

## Using `IntoKubeconfig`

To get a serialisable kubeconfig structure (e.g. to write `~/.kube/config`):

```rust,no_run
use kube_gke_config::{TryGkeClusterExt, IntoKubeconfig, default_gke_client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let gke = default_gke_client().await?;

    let cluster = gke
        .try_gke_cluster("my-project", "us-central1", "my-cluster")
        .await?;

    let kubeconfig = cluster.into_kubeconfig()?;
    let yaml = serde_yaml::to_string(&kubeconfig)?;
    println!("{yaml}");
    Ok(())
}
```


## GKE credentials

`default_gke_client()` resolves credentials via the standard Google ADC chain (highest
priority first):

| Source | How to configure |
|---|---|
| `GOOGLE_APPLICATION_CREDENTIALS` | Set to path of a service account JSON key |
| User credentials | Run `gcloud auth application-default login` |
| Metadata server | Automatic on Compute Engine, GKE, Cloud Run, etc. |

For custom credential configuration use
[`ClusterManager::builder().with_credentials(…).build().await?`](https://docs.rs/google-cloud-gax/latest/google_cloud_gax/client_builder/struct.ClientBuilder.html#method.with_credentials)
and then call [`TryGkeClusterExt`] methods on the resulting client.

## Traits

| Trait | Implemented on | Returns |
|---|---|---|
| `TryGkeClusterExt` | `ClusterManager` | `Cluster` / `Config` / `Client` |
| `ToKubeConfig` | `gke::model::Cluster` | `kube::Config` |
| `IntoKubeconfig` | `gke::model::Cluster` | `kube::config::Kubeconfig` |

## Examples

| Example | Description |
|---|---|
| `getpods-gke` | List pods in the default namespace of a GKE cluster |

```text
cargo run --example getpods-gke -- <project> <location> <cluster-name>
```

## Versioning

This crate tracks the major version of [`kube`](https://crates.io/crates/kube).
`kube-gke-config 3.x` is compatible with `kube 3.x`.

## License

Apache-2.0
