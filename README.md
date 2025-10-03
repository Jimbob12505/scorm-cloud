# SCORM Runtime Server

A minimal SCORM **runtime** service written in **Rust** (Axum + SQLx + Postgres) that lets you:

* Upload SCORM packages (ZIP)
* Parse `imsmanifest.xml` to register Course + SCOs
* Serve content files directly from disk
* Launch an in‑browser **SCORM 1.2 API** player shell (`window.API`)
* Track learner attempts and persist **CMI** data on commit

> ⚠️ This project is a functional prototype: the runtime currently implements a **subset** of SCORM 1.2 and uses a lightweight player shell. See **SCORM Support** and **Roadmap** below.

---

## Contents

* [Architecture](#architecture)
* [Directory Structure](#directory-structure)
* [Prerequisites](#prerequisites)
* [Quick Start (Docker Compose)](#quick-start-docker-compose)
* [Configuration (Environment Variables)](#configuration-environment-variables)
* [Build & Run From Source](#build--run-from-source)
* [Data Model](#data-model)
* [API Reference](#api-reference)

  * [Upload a SCORM package](#post-apicoursesupload)
  * [Create an attempt](#post-apiattempts)
  * [Launch the player](#get-playerattempt_id)
  * [Runtime endpoints](#runtime-endpoints)
* [SCORM Support](#scorm-support)
* [Security & Hardening](#security--hardening)
* [Troubleshooting](#troubleshooting)
* [Development Notes](#development-notes)
* [Roadmap](#roadmap)
* [FAQ](#faq)

---

## Architecture

**Runtime service** (Rust/Axum) exposes REST endpoints and serves static course files from a data directory. After upload, the service extracts the ZIP to `DATA_DIR/courses/<course_uuid>/`, parses `imsmanifest.xml`, and stores course metadata. A simple HTML player (`/player/:attempt_id`) loads the SCO in an `<iframe>` and injects **SCORM 1.2 API** (window.API) to talk back to the runtime endpoints.

**Key components**

* **Axum HTTP server** – routing, multipart handling, static file serving
* **SQLx (Postgres)** – courses, scos, attempts, cmi_values
* **quick-xml** – streaming parser for `imsmanifest.xml`
* **zip** – extraction of uploaded packages
* **tower-http** – CORS, compression, body limits, tracing

---

## Directory Structure

```
project-root/
├─ Cargo.toml               # Rust dependencies & metadata
├─ Dockerfile               # Build container for the app
├─ docker-compose.yml       # App + Postgres stack
├─ migrations/
│  └─ 0001_init.sql         # Tables: courses, scos, attempts, cmi_values
├─ src/
│  ├─ main.rs               # App bootstrap, router, layers
│  ├─ routes.rs             # HTTP endpoints & static serving
│  ├─ manifest.rs           # SCORM manifest parsing helpers
│  ├─ runtime.rs            # SCORM 1.2 runtime validation + helpers
│  ├─ models.rs             # (Course, SCO, Attempt, CmiValue) types
│  ├─ db.rs                 # SQLx pool setup
│  └─ util.rs               # misc helpers (URL encoding, etc.)
└─ data/                    # (created at runtime) extracted courses & uploads
```

> Note: You may also see a `target/` directory if you built the project locally (compiled artifacts).

---

## Prerequisites

* **Docker** and **Docker Compose** (recommended for quickest start), or
* **Rust** (1.75+ recommended) and **PostgreSQL 15+** if building locally

---

## Quick Start (Docker Compose)

1. **Clone** the repo and navigate to it.
2. **Create an `.env`** (optional) to set overrides (see [Configuration](#configuration-environment-variables)).
3. **Start the stack**:

```bash
docker compose up --build
```

This launches:

* `db` (Postgres, default user `postgres` / password `postgres`)
* `app` (Axum server) exposed on `http://localhost:8081`

4. **Health check**

Open: `http://localhost:8081/` (you should see a basic message or 404 if no root handler is provided). API endpoints start at `/api/...`.

---

## Configuration (Environment Variables)

| Variable           | Default                                                | Description                                          |
| ------------------ | ------------------------------------------------------ | ---------------------------------------------------- |
| `PORT`             | `8081`                                                 | HTTP server port                                     |
| `DATABASE_URL`     | `postgres://postgres:postgres@db:5432/scorm` (compose) | Postgres connection string                           |
| `DATA_DIR`         | `./data`                                               | Root directory for extracted courses and uploads     |
| `RUST_LOG`         | `info,axum=info,tower_http=info`                       | Logging configuration                                |
| `MAX_UPLOAD_BYTES` | `2147483648` (2 GiB)                                   | Max request size for uploads (if configured in code) |

> When running outside Docker, set `DATABASE_URL` to your local Postgres (e.g., `postgres://user:pass@localhost:5432/scorm`). Ensure the `migrations/` are applied (SQLx will run or you can run them manually).

---

## Build & Run From Source

1. **Install Rust**: [https://rustup.rs](https://rustup.rs)
2. **Start Postgres** and create a DB (e.g., `scorm`).
3. **Set env** and run:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/scorm
export DATA_DIR=./data
export PORT=8081

# First time: apply migrations (use your preferred tool) or let the app init
cargo run
```

The server listens on `http://127.0.0.1:8081` (or `0.0.0.0:8081` in Docker).

---

## Data Model

**Tables** (from `migrations/0001_init.sql`):

* `courses(id, title, org_identifier?, launch_href, base_path, created_at)`
* `scos(id, course_id→courses.id, identifier, launch_href, parameters?, created_at)`
* `attempts(id, course_id, learner_id, sco_id?, status, started_at, finished_at?, created_at)`
* `cmi_values(attempt_id, element, value, updated_at)` with UPSERT on commit

**Concepts**

* **Course**: one uploaded SCORM package; `base_path` points to the extracted directory under `DATA_DIR`.
* **SCO**: a launchable item resolved from `imsmanifest.xml` (`identifierref` → `resource@href` + `parameters`).
* **Attempt**: a learner’s session against a course (optionally a specific SCO).
* **CMI values**: key/value store for SCORM 1.2 elements (e.g., `cmi.core.lesson_status`).

---

## API Reference

### `POST /api/courses/upload`

**Description:** Upload a SCORM ZIP. The server extracts it to `DATA_DIR/courses/<uuid>/`, parses `imsmanifest.xml`, stores a Course row and SCO rows, and returns course metadata.

**Request (multipart/form-data):**

* `title` *(string, optional)* – display name; if omitted, derived from package
* `file` *(file, required)* – SCORM ZIP (must include `imsmanifest.xml` at root or nested under the package root)

**Example:**

```bash
curl --http1.1 \
  -F 'title=My Course' \
  -F 'file=@./course.zip;type=application/zip' \
  http://localhost:8081/api/courses/upload
```

**Response (JSON, example):**

```json
{
  "id": "2b2f2f6b-...",
  "title": "My Course",
  "base_path": "courses/2b2f2f6b-.../",
  "launch_href": "index.html",
  "scos": [
    { "id": "...", "identifier": "SCO-1", "launch_href": "sco1/index.html", "parameters": null }
  ]
}
```

---

### `POST /api/attempts`

**Description:** Create a learner attempt for a course (optionally targeting a specific SCO).

**Request (JSON):**

```json
{
  "course_id": "<uuid>",
  "learner_id": "user-123",
  "sco_id": "<uuid>"   // optional
}
```

**Example:**

```bash
curl -X POST http://localhost:8081/api/attempts \
  -H 'Content-Type: application/json' \
  -d '{"course_id":"<uuid>","learner_id":"user-123"}'
```

**Response:** the created Attempt row (JSON).

---

### `GET /player/:attempt_id`

**Description:** Returns an HTML page that launches the resolved SCO in an `<iframe>` and exposes **SCORM 1.2 API** as `window.API` for the content.

Open in a browser (after you create an attempt):

```
http://localhost:8081/player/<attempt_id>
```

The player determines the launch URL from the Course/SCO metadata, e.g.:

```
/content/<base_path>/<href>[?<parameters>]
```

---

### Runtime endpoints

These are called by the in‑page **SCORM API shim** (window.API). You typically won’t call them directly unless testing.

#### `POST /runtime/:attempt_id/initialize`

* Returns all known CMI values for the attempt.
* Body: `{}`
* Example:

```bash
curl -X POST http://localhost:8081/runtime/<attempt_id>/initialize -H 'Content-Type: application/json' -d '{}'
```

#### `POST /runtime/:attempt_id/set`

* **Prototype**: currently a **stub** that acknowledges but does not persist. Values are cached client-side until `commit`.
* Body: `{ "element": "cmi.core.lesson_location", "value": "page-3" }`

#### `POST /runtime/:attempt_id/get`

* **Prototype**: currently a **stub** returning `{ value: "" }`. Player uses local cache until `commit`.
* Body: `{ "element": "cmi.core.lesson_location" }`

#### `POST /runtime/:attempt_id/commit`

* Persists the client‑side cache into `cmi_values` with UPSERT; validates allowed elements and normalizes `lesson_status`.
* Body: `{ "values": { "cmi.core.lesson_status": "completed", "cmi.suspend_data": "..." } }`

#### `POST /runtime/:attempt_id/finish`

* Marks the attempt as completed and sets `finished_at`.
* Body: `{}`

---

## SCORM Support

**Implemented (SCORM 1.2 subset)**

* API entry points via `window.API` in the player shell
* Core elements (examples):

  * `cmi.core.lesson_status`
  * `cmi.core.lesson_location`
  * `cmi.core.score.raw`
  * `cmi.core.session_time`
  * `cmi.core.exit`
  * `cmi.suspend_data`
* Validation for element names and basic length constraints
* Persist-on-commit model (values written on `commit`)

**Not yet implemented / Partial**

* Full SCORM 1.2 error model (`LMSGetLastError`, `LMSGetErrorString`, …)
* Immediate persistence on `set`/`get`
* Full datatype and range validation for all elements
* SCORM 2004 organizations/sequencing
* Multi-SCO TOC and navigation UI in the player

---

## Security & Hardening

* **CORS**: default is permissive for development. In production, restrict origins, methods, and headers.
* **Auth**: add authentication (JWT/session) for upload, attempt creation, and runtime calls.
* **ZIP extraction**: sanitize paths to prevent traversal (`..`, absolute paths). Reject dangerous entries.
* **Body limits**: set `MAX_UPLOAD_BYTES` and return `413` for oversized payloads.
* **Disk quotas**: ensure `DATA_DIR` has sufficient space; rotate and clean stale attempts/uploads.
* **TLS/Proxy**: terminate TLS at a reverse proxy (Nginx/Caddy) or in-app via TLS if needed.

---

## Troubleshooting

* **"Error parsing multipart/form-data" on upload**

  * Usually a body-size limit. Increase `MAX_UPLOAD_BYTES` and the Axum body-limit layers; also configure proxy limits (e.g., `client_max_body_size` in Nginx).
  * Test with a small ZIP to verify field names: `title`, `file`.

* **Player loads but content is blank**

  * Check that `launch_href` exists under `/content/<base_path>/...` and the browser console for 404s.

* **CMI values not appearing after reload**

  * Prototype uses client-side cache; values persist only on `commit`. Ensure the SCO calls `Commit()` before exit.

* **Manifest not found**

  * Ensure `imsmanifest.xml` is present at the package root (or first directory level). Check casing and ZIP structure.

* **Database errors**

  * Verify `DATABASE_URL` and that migrations applied. In Docker Compose, DB is `db:5432`.

---

## Development Notes

* **Logging**: set `RUST_LOG=debug` for verbose traces (request IDs, route timing, commit payloads).
* **Hot reload**: use `cargo watch -x run` during local dev.
* **Testing uploads**: use `curl --http1.1 -F 'title=...' -F 'file=@./pkg.zip;type=application/zip' ...`.
* **Static serving**: `/content` is mounted to `DATA_DIR` using `ServeDir` (tower-http). Extracted courses live under `DATA_DIR/courses/<uuid>/`.

---

## Roadmap

* Implement server-side `set/get` (persist per element immediately)
* Expand SCORM 1.2 element coverage + full error model
* Add player TOC and multi‑SCO navigation
* (Optional) SCORM 2004 organizations & sequencing rules
* ZIP validation + antivirus hook (ClamAV) for enterprise deployments
* API tokens and role-based access control
* Export/reporting endpoints (attempt summaries, progress, scores)

---

## FAQ

**Q: Does it support SCORM 2004?**
A: Not yet. The current focus is SCORM 1.2 (subset). 2004 sequencing is on the roadmap.

**Q: Where are files stored?**
A: In `DATA_DIR`. Each upload extracts to `DATA_DIR/courses/<uuid>/` and content is served from `/content`.

**Q: Can I upload very large ZIPs?**
A: Yes, if you raise the body limits and proxy limits. See **Security & Hardening** and **Troubleshooting**.

**Q: How do I reset the DB?**
A: Stop the stack, remove the Postgres volume, and `docker compose up --build` again. Or drop/recreate the database locally.

**Q: How do I add auth?**
A: Put the app behind a reverse proxy (e.g., Nginx) with OpenID Connect, or add JWT/session middleware in Axum and protect routes.

