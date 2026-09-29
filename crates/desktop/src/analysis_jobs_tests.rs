use crate::DesktopClustering;
use archaeodash_application::ImportService;
use archaeodash_contracts::{
    AnalysisJobRequest, AnalysisJobResult, AnalysisJobSnapshot, AnalysisJobState,
    AnalysisSourceDto, ClusterDistanceMetricDto, ClusterFitRequest, ClusterLinkageDto,
    ClusterMethod, ImportCommitRequest, SubmitAnalysisJobRequest,
};

fn fixture() -> (std::path::PathBuf, String) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "archaeodash-desktop-job-test-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("project tempdir");
    std::fs::write(
        dir.join("mini.csv"),
        "anid,Site,as,fe\nA1,A,1,3\nA2,A,2,4\nA3,A,5,8\n",
    )
    .expect("write source");
    let import = ImportService::new(&dir).expect("import service");
    let result = import
        .commit(&ImportCommitRequest {
            source: "mini.csv".into(),
            group_column: "Site".into(),
            visible_id_column: None,
            elemental_columns: None,
            recipe: None,
            destination_dir: None,
            group_name: None,
        })
        .expect("commit source");
    (dir, result.groups[0].path.clone())
}

fn request(path: String, pc_count: Option<u32>) -> SubmitAnalysisJobRequest {
    SubmitAnalysisJobRequest {
        analysis: AnalysisJobRequest::ClusterFit(ClusterFitRequest {
            path,
            columns: vec!["as".into(), "fe".into()],
            transformation: None,
            source: AnalysisSourceDto::Pca,
            pc_count,
            source_group_column: None,
            umap_seed: None,
            metric: ClusterDistanceMetricDto::Manhattan,
            minkowski_p: 2.0,
            linkage: ClusterLinkageDto::Average,
            plot_group_column: None,
            method: ClusterMethod::Hclust,
            k: None,
            iter_max: 100,
            nstart: 25,
            seed: None,
        }),
        timeout_ms: Some(30_000),
    }
}

fn wait_for(clustering: &DesktopClustering, id: &str) -> AnalysisJobSnapshot {
    for _ in 0..200 {
        let snapshot = clustering
            .get_analysis_job(id)
            .expect("job is project-visible");
        if matches!(
            snapshot.state,
            AnalysisJobState::Succeeded
                | AnalysisJobState::Failed
                | AnalysisJobState::Cancelled
                | AnalysisJobState::TimedOut
        ) {
            return snapshot;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("desktop analysis job did not finish");
}

#[test]
fn desktop_jobs_preserve_project_scope_and_typed_nondefault_analysis_result() {
    let (first, path) = fixture();
    let desktop = DesktopClustering::new();
    desktop.open_project(first.clone()).unwrap();
    let submitted = desktop.submit_analysis_job(request(path, Some(1))).unwrap();
    let complete = wait_for(&desktop, &submitted.id);
    assert_eq!(complete.state, AnalysisJobState::Succeeded);
    let Some(AnalysisJobResult::ClusterFit(result)) = complete.result else {
        panic!("cluster fit result expected")
    };
    assert_eq!(result.source, AnalysisSourceDto::Pca);
    assert_eq!(result.metric, ClusterDistanceMetricDto::Manhattan);
    assert_eq!(result.linkage, ClusterLinkageDto::Average);
    assert_eq!(result.column_names, vec!["PC1"]);

    let (second, _) = fixture();
    desktop.open_project(second.clone()).unwrap();
    assert!(
        desktop.get_analysis_job(&submitted.id).is_err(),
        "job ID is isolated to its project instance"
    );
    assert!(desktop.get_analysis_job("missing-job-id").is_err());
}

#[test]
fn desktop_jobs_report_control_errors_and_cancel_endpoint_state() {
    let (project, path) = fixture();
    let desktop = DesktopClustering::new();
    desktop.open_project(project.clone()).unwrap();
    let bad = desktop
        .submit_analysis_job(request(path.clone(), Some(99)))
        .unwrap();
    let failed = wait_for(&desktop, &bad.id);
    assert_eq!(failed.state, AnalysisJobState::Failed);
    assert_eq!(failed.error.unwrap().code, "cluster_pc_count");

    let submitted = desktop.submit_analysis_job(request(path, Some(1))).unwrap();
    let cancelled = desktop.cancel_analysis_job(&submitted.id).unwrap();
    let cancelled = wait_for(&desktop, &cancelled.id);
    assert!(matches!(
        cancelled.state,
        AnalysisJobState::Cancelled | AnalysisJobState::Succeeded
    ));
    assert!(desktop.cancel_analysis_job("missing-job-id").is_err());
}
