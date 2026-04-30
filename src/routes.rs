use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::vfs::VirtualFilesystem;

pub fn router(filesystem: VirtualFilesystem) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/{*path}", get(path_handler))
        .with_state(filesystem)
}

async fn root(State(filesystem): State<VirtualFilesystem>) -> Html<String> {
    Html(render_directory_page(
        "/",
        None,
        &filesystem.root_listing().children,
    ))
}

async fn path_handler(
    State(filesystem): State<VirtualFilesystem>,
    Path(path): Path<String>,
) -> Response {
    let normalized = normalize_path(&path);

    if let Some(directory) = filesystem.directory_listing(&normalized) {
        Html(render_directory_page(
            &directory.path,
            parent_directory_path(&directory.path).as_deref(),
            &directory.children,
        ))
        .into_response()
    } else if let Some(file) = filesystem.file_entry(&normalized) {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            file.content,
        )
            .into_response()
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
) -> String {
    let mut markup = String::new();
    markup.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    markup.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    markup.push_str("<title>Index of ");
    markup.push_str(&html_escape(path));
    markup.push_str("</title>");
    markup.push_str(
        "<style>body{font-family:Arial,sans-serif;background:#f7f7f2;color:#222;margin:0;padding:24px} \
        .card{max-width:960px;margin:0 auto;background:#fff;border:1px solid #d7d7d1;border-radius:8px;box-shadow:0 8px 24px rgba(0,0,0,.06);overflow:hidden} \
        .header{padding:18px 24px;background:linear-gradient(180deg,#fefefe,#f1f1ea);border-bottom:1px solid #ddd} \
        h1{margin:0;font-size:1.4rem} \
        .path{margin-top:6px;color:#666;font-size:.95rem;word-break:break-all} \
        table{width:100%;border-collapse:collapse} \
        th,td{text-align:left;padding:12px 24px;border-bottom:1px solid #ecece6} \
        th{font-size:.8rem;text-transform:uppercase;letter-spacing:.08em;color:#666;background:#fafaf7} \
        a{color:#174ea6;text-decoration:none} a:hover{text-decoration:underline} \
        .meta{color:#666;font-size:.92rem} \
        .up{padding:16px 24px} \
        </style></head><body>",
    );
    markup.push_str("<div class=\"card\"><div class=\"header\"><h1>Index of ");
    markup.push_str(&html_escape(path));
    markup.push_str(
        "</h1><div class=\"path\">Browsing a seeded virtual filesystem mirror</div></div>",
    );
    markup.push_str("<div class=\"up\">");
    if let Some(parent_path) = parent_path {
        markup.push_str("<a href=\"");
        markup.push_str(&html_escape(parent_path));
        markup.push_str("\">Parent directory</a>");
    } else {
        markup.push_str("<span class=\"meta\">Parent directory</span>");
    }
    markup.push_str("</div>");
    markup.push_str("<table><thead><tr><th>Name</th><th>Type</th></tr></thead><tbody>");

    for child in children {
        let suffix = if child.is_directory { "/" } else { "" };
        markup.push_str("<tr><td><a href=\"");
        markup.push_str(&html_escape(&child.path));
        markup.push_str(suffix);
        markup.push_str("\">");
        markup.push_str(&html_escape(&child.name));
        markup.push_str(suffix);
        markup.push_str("</a></td><td class=\"meta\">");
        markup.push_str(if child.is_directory {
            "Directory"
        } else {
            "File"
        });
        markup.push_str("</td></tr>");
    }

    if children.is_empty() {
        markup.push_str("<tr><td colspan=\"2\" class=\"meta\">This directory is empty.</td></tr>");
    }

    markup.push_str("</tbody></table></div></body></html>");
    markup
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
