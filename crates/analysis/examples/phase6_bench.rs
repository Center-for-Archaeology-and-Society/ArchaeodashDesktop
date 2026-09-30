//! Reproducible Phase 6 numerical and cancellation benchmark.
//!
//! Run with: `cargo run --release -p archaeodash-analysis --example phase6_bench`

use std::{hint::black_box, thread, time::Instant};

use archaeodash_analysis::{
    calc_e_distance_cancellable, diana_with_metric, diana_with_metric_cancellable, get_eligible,
    group_mem_probs_tracked, group_mem_probs_tracked_cancellable, hclust, hclust_cancellable,
    kmeans, kmeans_cancellable, pam_with_metric, pam_with_metric_cancellable, CancellationToken,
    ColumnMatrix, DistanceMetric, LinkageMethod, MembershipMethod,
};
use archaeodash_domain::DomainError;

fn matrix(n: usize, p: usize) -> ColumnMatrix {
    let cols = (0..p)
        .map(|j| {
            (0..n)
                .map(|i| {
                    let residue = (i * (j + 3) + i * i % 997) % 104_729;
                    (residue as f64 / 100.0) + (j as f64 * 0.125)
                })
                .collect()
        })
        .collect();
    ColumnMatrix {
        names: (0..p).map(|j| format!("v{j}")).collect(),
        cols,
    }
}

fn measure<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = black_box(f());
    println!("{label}: {:.3}s", start.elapsed().as_secs_f64());
    out
}

fn cancellation_status(result: &Result<(), DomainError>) -> Result<&'static str, &DomainError> {
    match result {
        Err(DomainError::Validation { code, .. }) if code == "analysis_cancelled" => {
            Ok("cancelled")
        }
        Ok(()) => Ok("completed before cancellation"),
        Err(error) => Err(error),
    }
}

fn cancel_probe(
    label: &str,
    work: impl FnOnce(CancellationToken) -> Result<(), DomainError> + Send + 'static,
) {
    let token = CancellationToken::new();
    let worker_token = token.clone();
    let worker = thread::spawn(move || work(worker_token));
    thread::sleep(std::time::Duration::from_millis(5));
    let requested = Instant::now();
    token.cancel();
    let result = worker.join().expect("worker panicked");
    let status = cancellation_status(&result)
        .unwrap_or_else(|error| panic!("{label} cancellation probe failed: {error}"));
    println!(
        "{label} cancellation latency: {:.3}ms ({status})",
        requested.elapsed().as_secs_f64() * 1_000.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_probe_only_counts_the_cancellation_error_code() {
        assert_eq!(
            cancellation_status(&Err(DomainError::validation(
                "analysis_cancelled",
                "stopped"
            )))
            .expect("recognized cancellation"),
            "cancelled"
        );
        assert_eq!(
            cancellation_status(&Ok(())).expect("completed result"),
            "completed before cancellation"
        );
        assert!(matches!(cancellation_status(&Err(DomainError::validation(
            "cluster_resource_limit",
            "too large"
        ))), Err(DomainError::Validation { code, .. }) if code == "cluster_resource_limit"));
    }
}

fn main() {
    println!(
        "Phase 6 benchmark; rustc={}, profile=release",
        option_env!("RUSTC_VERSION").unwrap_or("record with rustc --version")
    );
    println!("synthetic deterministic matrix; p=8 for clustering, p=4 for membership/distances");
    for n in [100usize, 500, 1_000] {
        println!("\nrows={n}");
        let x = matrix(n, 8);
        measure("kmeans k=5 nstart=10 iter=100", || {
            kmeans(&x, 5, 100, 10, 20260928).expect("kmeans")
        });
        measure("PAM Manhattan k=5", || {
            pam_with_metric(&x, 5, DistanceMetric::Manhattan).expect("pam")
        });
        measure("HCA Euclidean Ward.D2", || {
            hclust(&x, DistanceMetric::Euclidean, LinkageMethod::WardD2).expect("hclust")
        });
        measure("HCA Maximum Complete", || {
            hclust(&x, DistanceMetric::Maximum, LinkageMethod::Complete).expect("hclust")
        });
        if n <= 500 || n == 1_000 {
            measure("DIANA Manhattan", || {
                diana_with_metric(&x, DistanceMetric::Manhattan).expect("diana")
            });
        }

        if n == 1_000 {
            let work = x.clone();
            cancel_probe("kmeans", move |token| {
                kmeans_cancellable(&work, 5, 1_000, 100, 20260928, &token).map(|_| ())
            });
            let work = x.clone();
            cancel_probe("PAM", move |token| {
                pam_with_metric_cancellable(&work, 5, DistanceMetric::Manhattan, &token).map(|_| ())
            });
            let work = x.clone();
            cancel_probe("HCA", move |token| {
                hclust_cancellable(
                    &work,
                    DistanceMetric::Maximum,
                    LinkageMethod::Complete,
                    &token,
                )
                .map(|_| ())
            });
            let work = x.clone();
            cancel_probe("DIANA", move |token| {
                diana_with_metric_cancellable(&work, DistanceMetric::Manhattan, &token).map(|_| ())
            });
        }
    }

    println!("\nrows=1500 (wide pure-analysis probe; service pairwise cap is 1000)");
    let wide = matrix(1_500, 8);
    measure("HCA Minkowski p=3 Average", || {
        hclust(
            &wide,
            DistanceMetric::Minkowski { p: 3.0 },
            LinkageMethod::Average,
        )
        .expect("hclust")
    });

    println!("\nprocedure 10/11 analysis calls, rows=1000");
    let chem = matrix(1_000, 4);
    let ids: Vec<String> = (0..1_000).map(|i| format!("id-{i}")).collect();
    let rowids: Vec<String> = (0..1_000).map(|i| format!("row-{i}")).collect();
    let groups: Vec<String> = (0..1_000)
        .map(|i| if i % 2 == 0 { "A" } else { "B" }.to_owned())
        .collect();
    let eligible = get_eligible(&groups, chem.cols.len());
    measure("membership Mahalanobis", || {
        group_mem_probs_tracked(
            &ids,
            &groups,
            "A",
            &chem,
            &chem.names,
            &eligible,
            MembershipMethod::Mahalanobis,
        )
        .expect("membership")
    });
    measure("Euclidean top-5", || {
        calc_e_distance_cancellable(
            &rowids,
            &ids,
            &groups,
            &chem,
            &eligible,
            5,
            false,
            &CancellationToken::new(),
        )
        .expect("euclidean")
    });

    let chem_clone = chem.clone();
    let ids_clone = ids.clone();
    let groups_clone = groups.clone();
    let eligible_clone = eligible.clone();
    cancel_probe("membership", move |token| {
        group_mem_probs_tracked_cancellable(
            &ids_clone,
            &groups_clone,
            "A",
            &chem_clone,
            &chem_clone.names,
            &eligible_clone,
            MembershipMethod::Mahalanobis,
            &token,
        )
        .map(|_| ())
    });
    let chem_clone = chem.clone();
    let ids_clone = ids.clone();
    let groups_clone = groups.clone();
    let rowids_clone = rowids.clone();
    let eligible_clone = eligible.clone();
    cancel_probe("Euclidean", move |token| {
        calc_e_distance_cancellable(
            &rowids_clone,
            &ids_clone,
            &groups_clone,
            &chem_clone,
            &eligible_clone,
            5,
            false,
            &token,
        )
        .map(|_| ())
    });
}
