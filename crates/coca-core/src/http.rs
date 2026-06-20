use std::io::{BufRead, Write};

use anyhow::{Context, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) reason: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: String,
}

impl HttpResponse {
    pub(crate) fn new(
        status: u16,
        reason: &'static str,
        content_type: &'static str,
        body: String,
    ) -> Self {
        Self {
            status,
            reason,
            content_type,
            body,
        }
    }
}

pub(crate) fn simple_response(
    status: u16,
    reason: &'static str,
    body: impl Into<String>,
) -> HttpResponse {
    HttpResponse::new(status, reason, "text/plain; charset=utf-8", body.into())
}

pub(crate) fn read_http_request(reader: &mut impl BufRead) -> Result<Option<HttpRequest>> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .context("failed to read HTTP request line")?
        == 0
    {
        return Ok(None);
    }

    let mut parts = line.split_whitespace();
    let Some(method) = parts.next().map(str::to_string) else {
        return Ok(None);
    };
    let Some(target) = parts.next().map(str::to_string) else {
        return Ok(None);
    };

    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader
            .read_line(&mut line)
            .context("failed to read HTTP headers")?
            == 0
        {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    Ok(Some(HttpRequest {
        method,
        target,
        headers,
    }))
}

pub(crate) fn write_http_response(writer: &mut impl Write, response: &HttpResponse) -> Result<()> {
    write!(
        writer,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
        response.body
    )
    .context("failed to write HTTP response")
}
