use axum::{
    extract::{Multipart, Path, State},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use sqlx::{query, query_as};
use std::path::PathBuf;
use tower_http::services::ServeDir;
use uuid::Uuid;
use axum::http::StatusCode;
use crate::{db::Db, manifest, models::*, runtime};

pub fn router(db: Db) -> Router {
    let static_dir = std::env::var("DATA_DIR").unwrap_or("./data".into());
    Router::new()
        // ingest + launch
        .route("/api/courses/upload", post(upload_course))
        .route("/api/attempts", post(create_attempt))
        .route("/player/:attempt_id", get(player_shell))
        // runtime API
        .route("/runtime/:attempt_id/initialize", post(rt_initialize))
        .route("/runtime/:attempt_id/set", post(rt_set))
        .route("/runtime/:attempt_id/get", post(rt_get))
        .route("/runtime/:attempt_id/commit", post(rt_commit))
        .route("/runtime/:attempt_id/finish", post(rt_finish))
        // static content (serves extracted course files)
        .nest_service("/content", ServeDir::new(static_dir))
        .with_state(db)
}

async fn upload_course(
    State(db): State<Db>,
    mut mp: Multipart,
) -> Result<Json<Course>, (axum::http::StatusCode, String)> {
    let mut title = None;
    let mut zip_bytes: Option<Vec<u8>> = None;

    while let Some(field) = mp.next_field().await.map_err(e500)? {
        let name = field.name().unwrap_or("").to_string();
        if name == "title" {
            title = Some(field.text().await.map_err(e500)?);
        } else if name == "file" {
            zip_bytes = Some(field.bytes().await.map_err(e500)?.to_vec());
        }
    }

    let title = title.unwrap_or_else(|| "Untitled Course".into());
    let bytes = zip_bytes.ok_or(e400("file is required"))?;

    let base_dir = PathBuf::from(std::env::var("DATA_DIR").unwrap_or("./data".into()));
    let course_id = Uuid::new_v4();
    let rel_base = format!("courses/{}", course_id);
    let out_dir = base_dir.join(&rel_base);

    manifest::extract_zip_to_dir(&bytes, &out_dir).map_err(e500)?;
    let mf = manifest::find_manifest(&out_dir).map_err(|_| e400("imsmanifest.xml not found"))?;
    let parsed = manifest::parse_manifest(&mf).map_err(|_| e400("failed to parse manifest"))?;

    // Persist course
    let course = query_as!(Course,
        r#"
        INSERT INTO courses (id, title, org_identifier, launch_href, base_path)
        VALUES ($1,$2,$3,$4,$5)
        RETURNING id, title, org_identifier, launch_href, base_path, created_at
        "#,
        course_id, title, Option::<String>::None, parsed.default_launch, rel_base
    )
    .fetch_one(&db)
    .await
    .map_err(e500)?;

    // Persist SCOs
    for (ident, href, params) in parsed.scos {
        let _ = query!(
            r#"INSERT INTO scos (course_id, identifier, launch_href, parameters) VALUES ($1,$2,$3,$4)"#,
            course.id, ident, href, params
        )
        .execute(&db)
        .await
        .map_err(e500)?;
    }

    Ok(Json(course))
}

async fn create_attempt(
    State(db): State<Db>,
    Json(req): Json<CreateAttemptReq>,
) -> Result<Json<Attempt>, (axum::http::StatusCode, String)> {
    let course: Option<Course> =
        query_as!(Course, "SELECT * FROM courses WHERE id=$1", req.course_id)
            .fetch_optional(&db)
            .await
            .map_err(e500)?;
    if course.is_none() {
        return Err(e400("course not found"));
    }

    let attempt_id = Uuid::new_v4();
    let rec = query_as!(Attempt,
        r#"
        INSERT INTO attempts (id, course_id, learner_id, sco_id, status, started_at)
        VALUES ($1,$2,$3,$4,'in_progress', now())
        RETURNING id, course_id, learner_id, sco_id, status, started_at, finished_at, created_at
        "#,
        attempt_id, req.course_id, req.learner_id, req.sco_id
    )
    .fetch_one(&db)
    .await
    .map_err(e500)?;

    Ok(Json(rec))
}

async fn player_shell(
    State(db): State<Db>,
    Path(attempt_id): Path<Uuid>,
) -> Result<Html<String>, (axum::http::StatusCode, String)> {
    let attempt: Attempt =
        query_as!(Attempt, "SELECT * FROM attempts WHERE id=$1", attempt_id)
            .fetch_one(&db)
            .await
            .map_err(e500)?;
    let course: Course =
        query_as!(Course, "SELECT * FROM courses WHERE id=$1", attempt.course_id)
            .fetch_one(&db)
            .await
            .map_err(e500)?;

    // Decide which href to launch
    let href = if let Some(sco_id) = attempt.sco_id {
        let sco: Sco = query_as!(Sco, "SELECT * FROM scos WHERE id=$1", sco_id)
            .fetch_one(&db)
            .await
            .map_err(e500)?;
        sco.launch_href
    } else {
        course.launch_href.clone()
    };

    // ServeDir is mounted at /content; base_path is relative to DATA_DIR
    let launch_url = format!("/content/{}/{}", course.base_path, href);

    let html = format!(
    r#"<!DOCTYPE html>
<html>
<head>
  <meta charset='utf-8'/>
  <title>SCORM Player</title>
  <meta http-equiv="Content-Security-Policy"
        content="default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self' blob:; font-src 'self' data:; frame-src 'self'; connect-src 'self';" />
  <style>
    html,body,iframe{{height:100%;width:100%;margin:0;padding:0;border:0}}
    .bar{{position:fixed;top:0;left:0;right:0;height:36px;background:#eee;border-bottom:1px solid #ddd;display:flex;align-items:center;padding:0 8px;z-index:2}}
    iframe{{position:absolute;top:36px;left:0;right:0;bottom:0}}
  </style>
</head>
<body>
<div class='bar'>Attempt {attempt_id} • <button onclick="console.log(window.APICommit())">Commit</button> <span id='status'></span></div>
<iframe id='sco' src='{launch_url}'></iframe>
<script>
(function(){{ 
  const cache = {{}};
  const attemptId = '{attempt_id}';

  async function post(path, body){{ 
    const res = await fetch(`/runtime/${{attemptId}}/${{path}}`, {{
      method:'POST',
      headers:{{'content-type':'application/json'}},
      body: JSON.stringify(body||{{}})
    }});
    const j = await res.json().catch(()=>({{}}));
    return j;
  }}

  async function initializeFromServer(){{ 
    try {{
      const j = await post('initialize');
      if (j && j.values && typeof j.values === 'object') {{
        Object.assign(cache, j.values);
      }}
    }} catch(e){{ console.warn('init failed', e); }}
  }}

  // SCORM 1.2 API shim
  window.API = {{
    LMSInitialize(arg){{ return "true"; }},
    LMSFinish(arg){{ post('finish'); return "true"; }},
    LMSGetValue(el){{ return (el in cache) ? String(cache[el]) : ""; }},
    LMSSetValue(el, v){{ cache[el]=String(v); return "true"; }},
    LMSCommit(arg){{ 
      post('commit', cache).then(()=>{{
        const s = document.getElementById('status');
        if (s){{ s.textContent='saved'; setTimeout(()=> s.textContent='', 1200); }}
      }});
      return "true";
    }},
    LMSGetLastError(){{ return "0"; }},
    LMSGetErrorString(c){{ return "No error"; }},
    LMSGetDiagnostic(c){{ return ""; }}
  }};

  // Seed cache before the SCO loads too far
  initializeFromServer();

  // toolbar helper
  window.APICommit = ()=> window.API.LMSCommit("");
}})();
</script>
</body>
</html>"#,
    attempt_id = attempt_id,
    launch_url = launch_url
);

    Ok(Html(html))
}

// --- Runtime endpoints (MVP) ---

async fn rt_initialize(
    State(db): State<Db>,
    Path(attempt_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let rows = sqlx::query!(
        r#"SELECT element, value FROM cmi_values WHERE attempt_id = $1"#,
        attempt_id
    )
    .fetch_all(&db)
    .await
    .map_err(e500)?;

    let mut map = serde_json::Map::new();
    for r in rows {
        // element is NOT NULL in schema; value may be NULL
        let v = r.value.unwrap_or_default();
        map.insert(r.element, serde_json::Value::String(v));
    }

    Ok(Json(serde_json::json!({ "values": map })))
}
async fn rt_set() -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true }))
}
async fn rt_get() -> impl IntoResponse {
    Json(serde_json::json!({ "value": "" }))
}

async fn rt_commit(
    State(db): State<Db>,
    Path(attempt_id): Path<Uuid>,
    Json(map): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let obj = map.as_object().cloned().unwrap_or_default();
    for (el, val) in obj.iter() {
    // Make an owned String so we never borrow a temporary.
        let value: String = val
            .as_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| val.to_string());

        if !runtime::is_valid_element_12(el) {
            continue;
        }
        if value.len() > runtime::max_len(el) {
            continue;
        }

        let v_final = if *el == "cmi.core.lesson_status" {
            runtime::normalize_lesson_status(&value)
                .unwrap_or("incomplete")
                .to_string()
        } else {
            value.clone()
        };

        let _ = query!(
            r#"
            INSERT INTO cmi_values (attempt_id, element, value)
            VALUES ($1,$2,$3)
            ON CONFLICT (attempt_id, element)
            DO UPDATE SET value=EXCLUDED.value, updated_at=now()
            "#,
            attempt_id,
            el,
            v_final
        )
        .execute(&db)
        .await
        .map_err(e500)?;
    }
 
    // Check completion status (deal with Option<Option<String>> from query_scalar+optional+nullable)
    let status: Option<String> = sqlx::query_scalar!(
        "SELECT value FROM cmi_values WHERE attempt_id=$1 AND element='cmi.core.lesson_status'",
        attempt_id
    )
    .fetch_optional(&db)
    .await
    .map_err(e500)?
    .flatten();

    if let Some(status) = status {
        if matches!(status.as_str(), "completed" | "passed" | "failed") {
            let _ = query!(
                "UPDATE attempts SET status='completed', finished_at=now() WHERE id=$1",
                attempt_id
            )
            .execute(&db)
            .await
            .map_err(e500)?;
        }
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn rt_finish(
    State(db): State<Db>,
    Path(attempt_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let _ = query!(
        "UPDATE attempts SET status='completed', finished_at=now() WHERE id=$1",
        attempt_id
    )
    .execute(&db)
    .await
    .map_err(e500)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- helpers ---
fn e400<T: Into<String>>(msg: T) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::BAD_REQUEST, msg.into())
}

fn e500<E: std::fmt::Display>(e: E) -> (axum::http::StatusCode, String) {
    tracing::error!(error=%e, "internal error");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

