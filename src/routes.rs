use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::{self, FormatItem};

use crate::vfs::VirtualFilesystem;

#[derive(Clone)]
pub struct AppState {
    pub filesystem: VirtualFilesystem,
    pub footer_signature: String,
    pub delay: Option<Duration>,
}

pub fn router(
    filesystem: VirtualFilesystem,
    footer_signature: String,
    delay: Option<Duration>,
) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/{*path}", get(path_handler))
        .with_state(AppState {
            filesystem,
            footer_signature,
            delay,
        })
}

async fn root(State(state): State<AppState>) -> Html<String> {
    if let Some(delay) = state.delay {
        tokio::time::sleep(delay).await;
    }
    Html(render_directory_page(
        "/",
        None,
        &state.filesystem.root_listing().await.children,
        &state.footer_signature,
    ))
}

async fn path_handler(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let normalized = normalize_path(&path);

    if let Some(delay) = state.delay {
        tokio::time::sleep(delay).await;
    }

    if let Some(directory) = state.filesystem.directory_listing(&normalized).await {
        Html(render_directory_page(
            &directory.path,
            parent_directory_path(&directory.path).as_deref(),
            &directory.children,
            &state.footer_signature,
        ))
        .into_response()
    } else if let Some(file) = state.filesystem.file_entry(&normalized).await {
        let file_name = file_name_for_download(&normalized).unwrap_or("download".to_string());
        let mut response = Response::new(Body::from_stream(file.stream));
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        if let Some(size) = file.size_bytes {
            if let Ok(value) = HeaderValue::from_str(&size.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, value);
            }
        }
        let disposition = format!("attachment; filename=\"{}\"", sanitize_filename(&file_name));
        if let Ok(value) = HeaderValue::from_str(&disposition) {
            response
                .headers_mut()
                .insert(header::CONTENT_DISPOSITION, value);
        }
        response
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", trimmed)
    }
}

fn render_directory_page(
    path: &str,
    parent_path: Option<&str>,
    children: &[crate::vfs::node::ChildEntry],
    footer_signature: &str,
) -> String {
    let time_format = format_description::parse("[day]-[month repr:short]-[year] [hour]:[minute]")
        .unwrap_or_default();
    let icon_dir = "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2216%22%20height%3D%2216%22%3E%3Cpath%20fill%3D%22%23c58c2a%22%20d%3D%22M1%204h6l2%202h6v8H1z%22/%3E%3Cpath%20fill%3D%22%23f2c14e%22%20d%3D%22M1%206h14v8H1z%22/%3E%3C/svg%3E";
    let icon_file = "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2216%22%20height%3D%2216%22%3E%3Cpath%20fill%3D%22%23e6e6e6%22%20stroke%3D%22%238a8a8a%22%20d%3D%22M3%201h7l3%203v11H3z%22/%3E%3Cpath%20fill%3D%22%23cfcfcf%22%20d%3D%22M10%201v3h3%22/%3E%3C/svg%3E";
    let icon_parent = "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2216%22%20height%3D%2216%22%3E%3Cpath%20fill%3D%22%236b6b6b%22%20d%3D%22M8%203l5%205H9v5H7V8H3z%22/%3E%3C/svg%3E";
    let mut markup = String::new();
    markup.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    markup.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    markup.push_str("<title>Index of ");
    markup.push_str(&html_escape(path));
    markup.push_str("</title>");
    markup.push_str("<style>");
    markup.push_str(
        "body{font-family:Verdana,Arial,Helvetica,sans-serif;background:#fff;color:#000;margin:0;padding:16px} \
        h1{font-size:1.4rem;margin:0 0 12px} \
        table{width:100%;border-collapse:collapse} \
        th,td{padding:3px 8px;text-align:left} \
        th{border-bottom:1px solid #aaa;font-weight:normal} \
        a{color:#00e;text-decoration:none} a:hover{text-decoration:underline} \
        .icon{width:20px} \
        .icon span{display:inline-block;width:16px;height:16px;background-size:16px 16px;background-repeat:no-repeat} \
        .icon.dir span{background-image:url('",
    );
    markup.push_str(icon_dir);
    markup.push_str("')} .icon.file span{background-image:url('");
    markup.push_str(icon_file);
    markup.push_str("')} .icon.parent span{background-image:url('");
    markup.push_str(icon_parent);
    markup.push_str(
        "')} .name{white-space:nowrap} .date{white-space:nowrap} .size{text-align:right} \
        hr{border:0;border-top:1px solid #aaa;margin:12px 0} \
        address{font-style:normal;color:#555;font-size:.9rem} \
        </style></head><body>",
    );
    markup.push_str("<h1>Index of ");
    markup.push_str(&html_escape(path));
    markup.push_str("</h1>");
    markup.push_str("<table><thead><tr>");
    markup.push_str(
        "<th class=\"icon\"></th><th>Name</th><th>Last modified</th><th class=\"size\">Size</th>",
    );
    markup.push_str("</tr></thead><tbody>");

    markup.push_str("<tr>");
    markup.push_str("<td class=\"icon parent\"><span></span></td>");
    markup.push_str("<td class=\"name\">");
    if let Some(parent_path) = parent_path {
        markup.push_str("<a href=\"");
        markup.push_str(&html_escape(parent_path));
        markup.push_str("\">Parent Directory</a>");
    } else {
        markup.push_str("Parent Directory");
    }
    markup.push_str("</td><td class=\"date\">-</td><td class=\"size\">-</td></tr>");

    for child in children {
        let suffix = if child.is_directory { "/" } else { "" };
        let icon_class = if child.is_directory { "dir" } else { "file" };
        let date_text = format_timestamp(child.modified_unix_seconds, &time_format);
        let size_text = format_size(child.size_bytes);
        markup.push_str("<tr>");
        markup.push_str("<td class=\"icon ");
        markup.push_str(icon_class);
        markup.push_str("\"><span></span></td><td class=\"name\"><a href=\"");
        markup.push_str(&html_escape(&child.path));
        markup.push_str(suffix);
        markup.push_str("\">");
        markup.push_str(&html_escape(&child.name));
        markup.push_str(suffix);
        markup.push_str("</a></td><td class=\"date\">");
        markup.push_str(&html_escape(&date_text));
        markup.push_str("</td><td class=\"size\">");
        markup.push_str(&html_escape(&size_text));
        markup.push_str("</td></tr>");
    }

    if children.is_empty() {
        markup.push_str("<tr><td colspan=\"4\">This directory is empty.</td></tr>");
    }

    markup.push_str("</tbody></table><hr><address>");
    markup.push_str(&html_escape(footer_signature));
    markup.push_str("</address></body></html>");
    markup
}

fn format_timestamp(value: Option<i64>, format: &[FormatItem<'_>]) -> String {
    let Some(seconds) = value else {
        return "-".to_string();
    };

    match OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(datetime) => datetime.format(format).unwrap_or_else(|_| "-".to_string()),
        Err(_) => "-".to_string(),
    }
}

fn format_size(value: Option<u64>) -> String {
    let Some(bytes) = value else {
        return "-".to_string();
    };

    if bytes < 1024 {
        return bytes.to_string();
    }

    let units = ["K", "M", "G", "T", "P"];
    let mut size = bytes as f64;
    let mut unit = "K";
    for next_unit in units {
        unit = next_unit;
        size /= 1024.0;
        if size < 1024.0 {
            break;
        }
    }

    if size < 10.0 {
        format!("{:.1}{}", size, unit)
    } else {
        format!("{:.0}{}", size, unit)
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn file_name_for_download(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        trimmed
            .rsplit_once('/')
            .map(|(_, name)| name.to_string())
            .or_else(|| Some(trimmed.to_string()))
    }
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '"' || ch == '\\' { '_' } else { ch })
        .collect()
}

fn parent_directory_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => Some(format!("/{}", parent)),
        _ => Some("/".to_string()),
    }
}
