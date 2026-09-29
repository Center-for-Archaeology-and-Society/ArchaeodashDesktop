use super::*;

use archaeodash_contracts::{
    AnalysisJobRequest, AnalysisJobResult, AnalysisJobSnapshot, AnalysisJobState,
    AnalysisSourceDto, ClusterDistanceMetricDto, ClusterFitRequest, ClusterLinkageDto,
    ClusterMethod, SubmitAnalysisJobRequest,
};

async fn get_snapshot(app: &Router, id: &str) -> AnalysisJobSnapshot {
    for _ in 0..200 {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(format!("/api/v1/jobs/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let snapshot = serde_json::from_slice::<AnalysisJobSnapshot>(&bytes).unwrap();
        if matches!(snapshot.state, AnalysisJobState::Succeeded | AnalysisJobState::Failed | AnalysisJobState::Cancelled | AnalysisJobState::TimedOut) {
            return snapshot;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("analysis job did not reach a terminal state");
}

fn fit(path: String, pc_count: Option<u32>) -> ClusterFitRequest {
    ClusterFitRequest {
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
    }
}

fn post_job(app: Router, request: SubmitAnalysisJobRequest) -> impl std::future::Future<Output = (StatusCode, AnalysisJobSnapshot)> {
    async move {
        let response = app
            .oneshot(
                axum::http::Request::post("/api/v1/jobs")
                    .header("content-type", "application/json")
                    .body(json_body(&request))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
}

#[tokio::test]
async fn jobs_recompute_pca_and_nondefault_hclust_then_report_bad_pc_count() {
    let (state, dir) = test_state();
    let app = root_router(state);
    let imported = commit_fixture(app.clone(), &dir).await;
    let path = imported.groups[0].path.clone();

    let (status, submitted) = post_job(
        app.clone(),
        SubmitAnalysisJobRequest {
            analysis: AnalysisJobRequest::ClusterFit(fit(path.clone(), Some(1))),
            timeout_ms: Some(30_000),
        },
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let complete = get_snapshot(&app, &submitted.id).await;
    assert_eq!(complete.state, AnalysisJobState::Succeeded);
    let Some(AnalysisJobResult::ClusterFit(result)) = complete.result else {
        panic!("typed cluster result expected")
    };
    assert_eq!(result.source, AnalysisSourceDto::Pca);
    assert_eq!(result.column_names, vec!["PC1"]);
    assert_eq!(result.metric, ClusterDistanceMetricDto::Manhattan);
    assert_eq!(result.linkage, ClusterLinkageDto::Average);
    assert_eq!(result.merge.as_ref().unwrap().len(), 1);

    let (status, submitted) = post_job(
        app.clone(),
        SubmitAnalysisJobRequest {
            analysis: AnalysisJobRequest::ClusterFit(fit(path, Some(99))),
            timeout_ms: Some(30_000),
        },
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let failed = get_snapshot(&app, &submitted.id).await;
    assert_eq!(failed.state, AnalysisJobState::Failed);
    assert_eq!(failed.error.unwrap().code, "cluster_pc_count");
}

#[tokio::test]
async fn jobs_reject_unknown_ids_and_project_escape_and_accept_cancel() {
    let (state, dir) = test_state();
    let app = root_router(state);
    let imported = commit_fixture(app.clone(), &dir).await;
    let path = imported.groups[0].path.clone();

    let missing = app.clone().oneshot(
        axum::http::Request::get("/api/v1/jobs/not-a-real-job").body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let (status, escaped) = post_job(app.clone(), SubmitAnalysisJobRequest {
        analysis: AnalysisJobRequest::ClusterFit(fit("../outside.parquet".into(), Some(1))),
        timeout_ms: Some(30_000),
    }).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let escaped = get_snapshot(&app, &escaped.id).await;
    assert_eq!(escaped.state, AnalysisJobState::Failed);

    let (status, submitted) = post_job(app.clone(), SubmitAnalysisJobRequest {
        analysis: AnalysisJobRequest::ClusterFit(fit(path, Some(1))),
        timeout_ms: Some(30_000),
    }).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let cancelled = app.clone().oneshot(
        axum::http::Request::post(format!("/api/v1/jobs/{}/cancel", submitted.id)).body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(cancelled.status(), StatusCode::OK);
    let body = cancelled.into_body().collect().await.unwrap().to_bytes();
    let snapshot: AnalysisJobSnapshot = serde_json::from_slice(&body).unwrap();
    let final_snapshot = get_snapshot(&app, &snapshot.id).await;
    assert!(matches!(final_snapshot.state, AnalysisJobState::Cancelled | AnalysisJobState::Succeeded));
}
