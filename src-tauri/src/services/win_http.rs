//! 用 Windows 自带的 WinHTTP 发 HTTPS 请求：走系统证书和系统代理（与浏览器、Codex 一致），
//! 不需要额外的网络库。只用于查询账号额度和刷新 token 这类小请求。

/// 发请求的函数签名；测试里替换成假的实现。
pub type HttpFn<'a> = dyn Fn(&str, &str, &[(&str, &str)], Option<&[u8]>) -> Result<HttpResponse, String> + 'a;

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn split_url(url: &str) -> Result<(bool, &str, u16, String), String> {
    let (is_https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("只支持 http 或 https 地址".into());
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return Err("请求地址缺少主机名".into());
    }
    let default_port = if is_https { 443 } else { 80 };
    let (host, port) = if let Some(after_bracket) = authority.strip_prefix('[') {
        let end = after_bracket.find(']').ok_or_else(|| "无效的 IPv6 地址".to_string())?;
        let host = &after_bracket[..end];
        let tail = &after_bracket[end + 1..];
        let port = match tail.strip_prefix(':') {
            Some(p) if !p.is_empty() => p.parse::<u16>().map_err(|_| "无效的端口号".to_string())?,
            Some(_) => return Err("无效的端口号".into()),
            None if tail.is_empty() => default_port,
            None => return Err("无效的主机地址".into()),
        };
        (host, port)
    } else if let Some((h, p)) = authority.split_once(':') {
        if h.is_empty() || p.is_empty() {
            return Err("无效的主机或端口".into());
        }
        let port = p.parse::<u16>().map_err(|_| "无效的端口号".to_string())?;
        (h, port)
    } else {
        (authority, default_port)
    };
    Ok((is_https, host, port, path))
}

/// 同步请求；支持 http / https 与自定义端口。超时 20 秒。
#[cfg(windows)]
pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<HttpResponse, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    let (is_https, host, port, path) = split_url(url)?;

    /// 关闭 WinHTTP 句柄。
    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }
    let check = |h: *mut core::ffi::c_void, what: &str| -> Result<Handle, String> {
        if h.is_null() {
            Err(format!("{}失败：{}", what, windows::core::Error::from_win32().message()))
        } else {
            Ok(Handle(h))
        }
    };

    unsafe {
        let session = check(
            WinHttpOpen(
                &HSTRING::from("ContextVault"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ),
            "初始化网络",
        )?;
        WinHttpSetTimeouts(session.0, 10_000, 10_000, 20_000, 20_000).map_err(|e| e.message())?;
        let connect = check(WinHttpConnect(session.0, &HSTRING::from(host), port, 0), "连接服务器")?;
        let flags = if is_https {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS::default()
        };
        let req = check(
            WinHttpOpenRequest(
                connect.0,
                &HSTRING::from(method),
                &HSTRING::from(path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                core::ptr::null(),
                flags,
            ),
            "创建请求",
        )?;
        let header_text: String = headers.iter().map(|(k, v)| format!("{}: {}\r\n", k, v)).collect();
        let header_wide: Vec<u16> = header_text.encode_utf16().collect();
        let body = body.unwrap_or(&[]);
        WinHttpSendRequest(
            req.0,
            if header_wide.is_empty() { None } else { Some(&header_wide) },
            if body.is_empty() { None } else { Some(body.as_ptr() as *const core::ffi::c_void) },
            body.len() as u32,
            body.len() as u32,
            0,
        )
        .map_err(|e| format!("发送请求失败：{}", e.message()))?;
        WinHttpReceiveResponse(req.0, core::ptr::null_mut()).map_err(|e| format!("等待响应失败：{}", e.message()))?;

        let mut status: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut core::ffi::c_void),
            &mut len,
            core::ptr::null_mut(),
        )
        .map_err(|e| format!("读取状态码失败：{}", e.message()))?;

        let mut out = Vec::new();
        loop {
            let mut available: u32 = 0;
            WinHttpQueryDataAvailable(req.0, &mut available).map_err(|e| format!("读取响应失败：{}", e.message()))?;
            if available == 0 {
                break;
            }
            let start = out.len();
            out.resize(start + available as usize, 0);
            let mut read: u32 = 0;
            WinHttpReadData(req.0, out[start..].as_mut_ptr() as *mut core::ffi::c_void, available, &mut read)
                .map_err(|e| format!("读取响应失败：{}", e.message()))?;
            out.truncate(start + read as usize);
            if out.len() > 4 * 1024 * 1024 {
                return Err("响应过大".into());
            }
        }
        Ok(HttpResponse { status: status as u16, body: out })
    }
}

#[cfg(not(windows))]
pub fn request(_method: &str, _url: &str, _headers: &[(&str, &str)], _body: Option<&[u8]>) -> Result<HttpResponse, String> {
    Err("当前系统不支持".into())
}

#[cfg(test)]
mod tests {
    /// 需要联网：cargo test win_http -- --ignored
    #[test]
    #[ignore]
    fn fetches_a_public_https_document_through_the_system_proxy() {
        let r = super::request("GET", "https://auth.openai.com/.well-known/openid-configuration", &[("Accept", "application/json")], None).unwrap();
        assert_eq!(r.status, 200);
        assert!(r.text().contains("token_endpoint"));
    }
}
