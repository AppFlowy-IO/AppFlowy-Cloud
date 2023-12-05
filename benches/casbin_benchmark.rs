use appflowy_cloud::biz::casbin::adapter::{create_collab_policies, create_grouping_policies};
use appflowy_cloud::biz::{
  casbin::access_control::CasbinAccessControl, casbin::MODEL_CONF, pg_listener::PgListeners,
};
use casbin::CoreApi;
use casbin::MgmtApi;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use database_entity::dto::AFAccessLevel;
use database_entity::pg_row::AFCollabMemerAccessLevelRow;
use dotenv::dotenv;
use futures::stream::{self};
use futures::StreamExt;
use rand::{rngs::ThreadRng, seq::SliceRandom, Rng};
use realtime::collaborate::CollabAccessControl;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

fn casbin_collab_access_control_benchmark(c: &mut Criterion) {
  dotenv().ok();

  let runtime = tokio::runtime::Builder::new_current_thread()
    .enable_time()
    .enable_io()
    .build()
    .unwrap();

  let pool = runtime.block_on(async {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPoolOptions::new()
      .connect(&db_url)
      .await
      .expect("failed to connect to db")
  });

  let pg_listeners = runtime.block_on(async {
    PgListeners::new(&pool)
      .await
      .expect("failed to create pg_listeners")
  });

  let mut group = c.benchmark_group("can_send_collab_update");
  group.measurement_time(std::time::Duration::from_secs(60));

  // size is the number of collabs and the number of members in each collab.
  for size in [100, 1000] {
    let mut enforcer = runtime.block_on(async {
      let model = casbin::DefaultModel::from_str(MODEL_CONF)
        .await
        .expect("failed to load model");
      casbin::Enforcer::new(model, casbin::NullAdapter)
        .await
        .expect("failed to create enforcer")
    });

    // setup policies
    runtime.block_on(async {
      let policies = create_grouping_policies();
      enforcer
        .add_grouping_policies(policies)
        .await
        .expect("failed to add grouping policies");
    });

    let mut rng = rand::thread_rng();

    // target that will be used to benchmark.
    let target_uid = 100_000;
    let target_oid = Uuid::new_v4().to_string();
    let target_access_level = AFAccessLevel::FullAccess;

    let target_member: Vec<sqlx::Result<AFCollabMemerAccessLevelRow>> =
      vec![Ok(AFCollabMemerAccessLevelRow {
        uid: target_uid,
        oid: target_oid.clone(),
        access_level: target_access_level,
      })];

    // generate policies
    runtime.block_on(async {
      let collab_members = gen_random_collab_members(&mut rng, size);
      let policies = create_collab_policies(stream::iter(collab_members).boxed())
        .await
        .expect("failed to generate collab policies");
      enforcer
        .add_policies(policies)
        .await
        .expect("failed to add collab policies");

      // add the policy that will be used to benchmark
      let target_policy = create_collab_policies(stream::iter(target_member).boxed())
        .await
        .expect("failed to generate collab policies");
      enforcer
        .add_policies(target_policy)
        .await
        .expect("failed to add collab policies");
    });

    let access_control = runtime.block_on(async {
      CasbinAccessControl::new(
        pool.clone(),
        pg_listeners.subscribe_collab_member_change(),
        pg_listeners.subscribe_workspace_member_change(),
        enforcer,
      )
    });
    let access_control = access_control.new_collab_access_control();

    group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
      b.to_async(&runtime).iter(|| async {
        access_control
          .can_send_collab_update(black_box(&target_uid), black_box(&target_oid))
          .await
          .expect("failed to check permission")
      })
    });
  }
  group.finish();
}

/// Generate random collabs and their members.
/// `n` is the number of collabs and members to generate.
/// Panics if `n` is less than 1.
fn gen_random_collab_members(
  rng: &mut ThreadRng,
  n: i64,
) -> Vec<sqlx::Result<AFCollabMemerAccessLevelRow>> {
  assert!(n > 0);
  let mut collab_members: Vec<sqlx::Result<AFCollabMemerAccessLevelRow>> = Vec::new();
  for _ in 0..n {
    let collab_id = Uuid::new_v4().to_string();
    for _ in 0..n {
      collab_members.push(Ok(AFCollabMemerAccessLevelRow {
        uid: rng.gen_range(0..n),
        oid: collab_id.clone(),
        access_level: gen_random_permission(rng).expect("failed to generate access level"),
      }));
    }
  }
  collab_members
}

/// Generate a random access level.
fn gen_random_permission(rng: &mut ThreadRng) -> Option<AFAccessLevel> {
  let levels = [
    AFAccessLevel::ReadOnly,
    AFAccessLevel::ReadAndComment,
    AFAccessLevel::ReadAndWrite,
    AFAccessLevel::FullAccess,
  ];
  levels.choose(rng).copied()
}

criterion_group!(benches, casbin_collab_access_control_benchmark);
criterion_main!(benches);
