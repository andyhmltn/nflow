use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeString};
use objc2_foundation::NSString;

use crate::hint::collect::AxElement;

pub fn copy_link(element: &AxElement) -> bool {
    match element.url() {
        Some(url) => {
            let display = element.link_text().unwrap_or_else(|| url.clone());
            let html = rich_link_html(&url, &display);
            write_rich(&html, &display);
            true
        }
        None => match element.link_text() {
            Some(title) => {
                write_plain(&title);
                true
            }
            None => false,
        },
    }
}

fn write_rich(html: &str, plain: &str) {
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(html), NSPasteboardTypeHTML);
        pb.setString_forType(&NSString::from_str(plain), NSPasteboardTypeString);
    }
}

fn write_plain(plain: &str) {
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(plain), NSPasteboardTypeString);
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn rich_link_html(url: &str, title: &str) -> String {
    format!(
        "<a href=\"{}\">{}</a>",
        escape_html(url),
        escape_html(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_title_and_url_in_anchor() {
        assert_eq!(
            rich_link_html("https://example.com/pr/1", "Fix the bug"),
            "<a href=\"https://example.com/pr/1\">Fix the bug</a>"
        );
    }

    #[test]
    fn escapes_ampersand_in_url() {
        assert_eq!(
            rich_link_html("https://x.com/?a=1&b=2", "PR"),
            "<a href=\"https://x.com/?a=1&amp;b=2\">PR</a>"
        );
    }

    #[test]
    fn escapes_angle_brackets_and_quotes_in_title() {
        assert_eq!(
            rich_link_html("https://x.com", "fix <script> \"now\""),
            "<a href=\"https://x.com\">fix &lt;script&gt; &quot;now&quot;</a>"
        );
    }
}
