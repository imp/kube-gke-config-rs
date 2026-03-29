//! Example: list pods in the default namespace of a GKE cluster.
//!
//! Google credentials are resolved automatically from the environment
//! (GOOGLE_APPLICATION_CREDENTIALS, `gcloud auth application-default login`,
//! or the GCE/GKE metadata server).
//!
//! Usage:
//! ```text
//! cargo run --example getpods-gke -- <project> <location> <cluster-name>
//! ```

use std::env;

use k8s_openapi::api::core::v1 as corev1;
use kube::api;
use kube_client::ResourceExt;
use kube_gke_config::TryGkeClusterExt;
use kube_gke_config::default_gke_client;

#[tokio::main]
async fn main() -> kube::Result<()> {
    let mut args = env::args();
    let cmd = args.next().unwrap();

    let (Some(project), Some(location), Some(cluster)) = (args.next(), args.next(), args.next())
    else {
        eprintln!("Usage: {cmd} <project> <location> <cluster-name>");
        return Ok(());
    };

    let cm = default_gke_client()
        .await
        .map_err(|err| kube::Error::Service(Box::new(err)))?;

    // Step 1 (optional): inspect the raw cluster metadata from GKE.
    let info = cm
        .try_gke_cluster(&project, &location, &cluster)
        .await
        .map_err(|err| kube::Error::Service(Box::new(err)))?;
    println!("Cluster endpoint   : {}", info.endpoint);
    println!("Kubernetes version : {}", info.current_master_version);

    // Step 2 (optional): inspect the derived kube config.
    let config = cm
        .try_gke_kube_config(&project, &location, &cluster)
        .await?;
    println!("kube::Config URL   : {}", config.cluster_url);

    // Step 3: build the client and list pods.
    let client = cm
        .try_gke_kube_client(&project, &location, &cluster)
        .await?;
    let lp = api::ListParams::default();
    let pods = api::Api::<corev1::Pod>::default_namespaced(client)
        .list(&lp)
        .await?;
    println!("\nPods in default namespace:");
    for pod in pods {
        println!("  {}", pod.name_any());
    }

    Ok(())
}
