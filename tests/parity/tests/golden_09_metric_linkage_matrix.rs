//! Expanded procedure-9 R oracle matrix for every supported HCA metric/linkage
//! and PAM/DIANA Euclidean/Manhattan on tied and non-tied inputs.
use archaeodash_analysis::{
    diana_with_metric, hclust, pam_with_metric, ColumnMatrix, DistanceMetric, LinkageMethod,
};

const FIXTURE: &str = include_str!("../../../fixtures/golden/09_metric_linkage_matrix.tsv");

fn data(name: &str) -> Vec<Vec<f64>> {
    match name {
        "tie" => vec![
            vec![0., 0.],
            vec![0., 0.],
            vec![1., 0.],
            vec![0., 1.],
            vec![1., 1.],
            vec![2., 1.],
        ],
        "plain" => vec![
            vec![0., 0.],
            vec![1., 2.],
            vec![2., 0.],
            vec![4., 1.],
            vec![5., 3.],
            vec![3., 4.],
        ],
        _ => panic!("unknown fixture data {name}"),
    }
}

fn matrix(rows: &[Vec<f64>]) -> ColumnMatrix {
    ColumnMatrix {
        names: vec!["x".into(), "y".into()],
        cols: (0..2)
            .map(|j| rows.iter().map(|r| r[j]).collect())
            .collect(),
    }
}
fn metric(s: &str) -> DistanceMetric {
    match s {
        "euclidean" => DistanceMetric::Euclidean,
        "manhattan" => DistanceMetric::Manhattan,
        "minkowski" => DistanceMetric::Minkowski { p: 3.0 },
        "maximum" => DistanceMetric::Maximum,
        _ => panic!("unknown metric {s}"),
    }
}
fn linkage(s: &str) -> LinkageMethod {
    match s {
        "average" => LinkageMethod::Average,
        "complete" => LinkageMethod::Complete,
        "ward.D" => LinkageMethod::WardD,
        "ward.D2" => LinkageMethod::WardD2,
        _ => panic!("unknown linkage {s}"),
    }
}
fn ints(s: &str) -> Vec<usize> {
    s.split(',')
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().unwrap())
        .collect()
}
fn merge_rows(s: &str) -> Vec<[i32; 2]> {
    s.split(';')
        .map(|row| {
            let v: Vec<i32> = row.split(',').map(|x| x.parse().unwrap()).collect();
            [v[0], v[1]]
        })
        .collect()
}
fn floats(s: &str) -> Vec<f64> {
    s.split(',')
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().unwrap())
        .collect()
}

#[test]
fn all_phase6_distance_linkage_combinations_match_r_oracle() {
    let mut hca = 0;
    let mut pam = 0;
    let mut diana = 0;
    for line in FIXTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("kind\t"))
    {
        let f: Vec<&str> = line.split('\t').collect();
        let x = matrix(&data(f[1]));
        match f[0] {
            "hca" => {
                let fit = hclust(&x, metric(f[2]), linkage(f[3])).unwrap();
                assert_eq!(
                    fit.merge,
                    merge_rows(f[4]),
                    "merge {} {} {}",
                    f[1],
                    f[2],
                    f[3]
                );
                assert_eq!(fit.order, ints(f[6]), "order {} {} {}", f[1], f[2], f[3]);
                let expected = floats(f[5]);
                assert_eq!(fit.height.len(), expected.len());
                for (i, (a, b)) in fit.height.iter().zip(expected).enumerate() {
                    assert!(
                        (a - b).abs() <= 2e-12 * (1.0 + b.abs()),
                        "height[{i}] {} {} {}: {a} != {b}",
                        f[1],
                        f[2],
                        f[3]
                    );
                }
                hca += 1;
            }
            "pam" => {
                let fit = pam_with_metric(&x, 2, metric(f[2])).unwrap();
                assert_eq!(fit.medoids, ints(f[7]), "medoids {} {}", f[1], f[2]);
                assert_eq!(
                    fit.clustering,
                    ints(f[8]).into_iter().map(|v| v as i32).collect::<Vec<_>>(),
                    "labels {} {}",
                    f[1],
                    f[2]
                );
                let obj = floats(f[9]);
                assert!((fit.build_objective - obj[0]).abs() < 1e-12);
                assert!((fit.swap_objective - obj[1]).abs() < 1e-12);
                pam += 1;
            }
            "diana" => {
                let fit = diana_with_metric(&x, metric(f[2])).unwrap();
                assert_eq!(fit.merge, merge_rows(f[4]), "DIANA merge {} {}", f[1], f[2]);
                assert_eq!(fit.order, ints(f[6]), "DIANA order {} {}", f[1], f[2]);
                let expected = floats(f[5]);
                assert_eq!(fit.height.len(), expected.len());
                for (i, (a, b)) in fit.height.iter().zip(expected).enumerate() {
                    assert!(
                        (a - b).abs() <= 2e-12 * (1.0 + b.abs()),
                        "DIANA height[{i}] {} {}: {a} != {b}",
                        f[1],
                        f[2]
                    );
                }
                diana += 1;
            }
            kind => panic!("unknown oracle kind {kind}"),
        }
    }
    assert_eq!((hca, pam, diana), (32, 4, 4));
}
