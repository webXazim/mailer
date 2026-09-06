pub(crate) struct AccountEmail {
    pub(crate) subject: &'static str,
    pub(crate) text: String,
    pub(crate) html: String,
}

pub(crate) fn verification(code: &str) -> AccountEmail {
    AccountEmail {
        subject: "Your Mailer verification code",
        text: format!(
            "Your CrescentSphere Mailer verification code is:\n\n{code}\n\nThis code expires in 15 minutes. If you did not create this account, you can safely ignore this email."
        ),
        html: layout(
            "Verify your email address",
            "Complete your CrescentSphere Mailer account setup.",
            &format!(
                r#"<p style="margin:0 0 24px;color:#475569;font-size:16px;line-height:1.65;">Enter this verification code to finish creating your account:</p>
<div style="margin:0 0 24px;padding:20px 16px;border:1px solid #dbeafe;border-radius:12px;background:#eff6ff;color:#0f172a;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:32px;font-weight:700;letter-spacing:8px;text-align:center;">{}</div>
<p style="margin:0;color:#64748b;font-size:14px;line-height:1.65;">This code expires in 15 minutes and can be used only once.</p>"#,
                escape_html(code)
            ),
            "If you did not create a CrescentSphere Mailer account, you can safely ignore this email.",
        ),
    }
}

pub(crate) fn password_reset(link: &str) -> AccountEmail {
    let escaped_link = escape_html(link);
    AccountEmail {
        subject: "Reset your Mailer password",
        text: format!(
            "Reset your CrescentSphere Mailer password using this link (valid for one hour):\n\n{link}\n\nIf you did not request this, you can safely ignore this email."
        ),
        html: layout(
            "Reset your password",
            "A password reset was requested for your CrescentSphere Mailer account.",
            &format!(
                r#"<p style="margin:0 0 24px;color:#475569;font-size:16px;line-height:1.65;">Use the button below to choose a new password. This secure link expires in one hour and can be used only once.</p>
<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:0 0 24px;"><tr><td style="border-radius:8px;background:#2563eb;"><a href="{escaped_link}" style="display:inline-block;padding:13px 22px;color:#ffffff;font-size:15px;font-weight:700;text-decoration:none;">Reset password</a></td></tr></table>
<p style="margin:0 0 8px;color:#64748b;font-size:13px;line-height:1.6;">If the button does not work, copy and paste this address into your browser:</p>
<p style="margin:0;color:#2563eb;font-size:12px;line-height:1.6;overflow-wrap:anywhere;word-break:break-all;">{escaped_link}</p>"#
            ),
            "If you did not request a password reset, you can safely ignore this email. Your password has not changed.",
        ),
    }
}

fn layout(title: &str, preheader: &str, content: &str, security_note: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light"><title>{}</title></head>
<body style="margin:0;padding:0;background:#f4f7fb;color:#0f172a;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Arial,sans-serif;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent;">{}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="width:100%;background:#f4f7fb;"><tr><td align="center" style="padding:32px 16px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="width:100%;max-width:600px;">
<tr><td style="padding:0 4px 20px;"><table role="presentation" cellpadding="0" cellspacing="0" border="0"><tr><td style="width:34px;height:34px;border-radius:8px;background:#0f172a;color:#ffffff;font-size:13px;font-weight:800;text-align:center;">CS</td><td style="padding-left:10px;color:#0f172a;font-size:16px;font-weight:750;">CrescentSphere <span style="color:#2563eb;">Mailer</span></td></tr></table></td></tr>
<tr><td style="padding:36px;border:1px solid #e2e8f0;border-radius:16px;background:#ffffff;box-shadow:0 8px 30px rgba(15,23,42,.06);">
<p style="margin:0 0 10px;color:#2563eb;font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;">Account security</p>
<h1 style="margin:0 0 18px;color:#0f172a;font-size:28px;line-height:1.25;letter-spacing:-.02em;">{}</h1>
{}
</td></tr>
<tr><td style="padding:20px 12px 0;color:#64748b;font-size:12px;line-height:1.65;text-align:center;">{}<br><span style="color:#94a3b8;">CrescentSphere Mailer · Developer email infrastructure</span></td></tr>
</table></td></tr></table>
</body></html>"#,
        escape_html(title),
        escape_html(preheader),
        escape_html(title),
        content,
        escape_html(security_note),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{password_reset, verification};

    #[test]
    fn verification_has_accessible_text_and_html_content() {
        let email = verification("180663");
        assert!(email.text.contains("180663"));
        assert!(email.html.contains("180663"));
        assert!(email.html.contains("15 minutes"));
        assert!(email.html.contains("role=\"presentation\""));
    }

    #[test]
    fn reset_link_is_escaped_in_html() {
        let email = password_reset("https://example.com/reset?a=1&b=\"two\"");
        assert!(email.text.contains("&b=\"two\""));
        assert!(email.html.contains("&amp;b=&quot;two&quot;"));
        assert!(!email
            .html
            .contains("href=\"https://example.com/reset?a=1&b="));
    }
}
