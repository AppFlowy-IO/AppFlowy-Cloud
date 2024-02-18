use actix_web::{web, Scope};

pub fn pprof_scope() -> Scope {
  web::scope("/debug/pprof")

    // Usage:
    // curl localhost:3000/debug/pprof/heap > heap.pb.gz
    // pprof -http=:8080 heap.pb.gz
    .service(web::resource("/heap").route(web::get().to(handle_get_heap)))
}

async fn handle_get_heap() -> Vec<u8> {
  let mut prof_ctl = match jemalloc_pprof::PROF_CTL.as_ref() {
    Some(x) => x.lock().await,
    None => return "none: cannot get prof_ctl".into(),
  };
  if !prof_ctl.activated() {
    return "heap profiling not activated".into();
  }
  match prof_ctl.dump_pprof() {
    Ok(x) => x,
    Err(e) => format!("error: {}", e).into(),
  }
}
