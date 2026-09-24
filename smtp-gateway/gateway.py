"""Authenticated SMTP submission adapter for the Mailer HTTP admission API."""

import asyncio
import base64
from email import policy
from email.parser import BytesParser
from email.utils import getaddresses
import hashlib
import json
import logging
import os
import ssl
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from aiosmtpd.controller import Controller
from aiosmtpd.smtp import AuthResult, MISSING


API_URL = os.environ.get("MAILER_INTERNAL_API", "http://api:8080").rstrip("/")
GATEWAY_SECRET = os.environ["SMTP_GATEWAY_SHARED_SECRET"]
CERT = os.environ.get("SMTP_GATEWAY_CERT", "/tls/fullchain.pem")
KEY = os.environ.get("SMTP_GATEWAY_KEY", "/tls/privkey.pem")
USERNAME = b"mailer"
MAX_MESSAGE_BYTES = 25_000_000
MAX_RECIPIENTS = 50


def api_request(path, key, payload=None, client_ip=None, idempotency=None):
    headers = {
        "Authorization": "Bearer " + key,
        "X-SMTP-Gateway-Secret": GATEWAY_SECRET,
    }
    if client_ip:
        headers["X-SMTP-Client-IP"] = client_ip
    if idempotency:
        headers["Idempotency-Key"] = idempotency
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    if data is not None:
        headers["Content-Type"] = "application/json"
    request = Request(API_URL + path, data=data or b"", headers=headers, method="POST")
    try:
        with urlopen(request, timeout=25) as response:
            return response.status, json.load(response)
    except HTTPError as error:
        # Never log credentials, message data, or the upstream error body.
        return error.code, None
    except (URLError, TimeoutError, OSError, ValueError):
        return 503, None


def one_address(value):
    addresses = getaddresses([value])
    if len(addresses) != 1:
        raise ValueError("one From address is required")
    address = addresses[0][1]
    if not address or "@" not in address or any(c in address for c in "\r\n"):
        raise ValueError("invalid From address")
    return address


def api_message(raw, sender, recipients):
    message = BytesParser(policy=policy.default).parsebytes(raw)
    if message.defects:
        raise ValueError("malformed MIME message")
    from_header = str(message.get("From", ""))
    if one_address(from_header).casefold() != sender.casefold():
        raise ValueError("From must match SMTP MAIL FROM")
    subject = str(message.get("Subject", ""))
    if not subject:
        raise ValueError("Subject is required")

    envelope = {address.casefold(): address for address in recipients}
    to_addresses = {addr.casefold() for _, addr in getaddresses(message.get_all("To", []))}
    cc_addresses = {addr.casefold() for _, addr in getaddresses(message.get_all("Cc", []))}
    to, cc, bcc = [], [], []
    for normalized, original in envelope.items():
        (to if normalized in to_addresses else cc if normalized in cc_addresses else bcc).append(original)

    text_parts, html_parts, attachments = [], [], []
    for part in message.walk():
        if part.is_multipart():
            continue
        disposition = part.get_content_disposition()
        filename = part.get_filename()
        if disposition in {"attachment", "inline"} or filename:
            if not filename or len(attachments) >= 10:
                raise ValueError("unsupported attachment")
            content = part.get_payload(decode=True)
            if content is None or len(content) > 10_000_000:
                raise ValueError("attachment exceeds limit")
            attachments.append({
                "filename": filename,
                "content": base64.b64encode(content).decode("ascii"),
                "content_type": part.get_content_type(),
                "content_disposition": disposition or "attachment",
                "content_id": str(part.get("Content-ID", "")).strip("<>") or None,
            })
        elif part.get_content_type() == "text/plain":
            text_parts.append(part.get_content())
        elif part.get_content_type() == "text/html":
            html_parts.append(part.get_content())
        else:
            raise ValueError("unsupported MIME content")
    if not text_parts and not html_parts:
        raise ValueError("text or HTML content is required")
    if sum(len(item.encode()) for item in text_parts + html_parts) > 1_000_000:
        raise ValueError("message content exceeds limit")
    if sum(len(base64.b64decode(item["content"])) for item in attachments) > 20_000_000:
        raise ValueError("attachments exceed limit")
    reply_to = message.get("Reply-To")
    return {
        "from": from_header,
        "to": to,
        "cc": cc,
        "bcc": bcc,
        "subject": subject,
        "text": "\n".join(text_parts) or None,
        "html": "\n".join(html_parts) or None,
        "reply_to": one_address(str(reply_to)) if reply_to else None,
        "attachments": attachments,
    }


class MailerHandler:
    async def _authorize(self, server, username, password):
        if username != USERNAME or not password.startswith((b"cs_live_", b"cs_test_")):
            await asyncio.sleep(0.2)
            return AuthResult(success=False, handled=False)
        try:
            key = password.decode("ascii")
        except UnicodeDecodeError:
            return AuthResult(success=False, handled=False)
        status, _ = await asyncio.to_thread(api_request, "/internal/v1/smtp/verify", key)
        if status == 200:
            server.session.mailer_key = key
            return AuthResult(success=True)
        await asyncio.sleep(0.2)
        return AuthResult(success=False, handled=False)

    async def auth_PLAIN(self, server, args):
        if len(args) == 1:
            decoded = await server.challenge_auth("")
            if decoded is MISSING:
                return AuthResult(success=False)
        else:
            try:
                decoded = base64.b64decode(args[1], validate=True)
            except (ValueError, base64.binascii.Error):
                await server.push("501 5.5.2 Invalid authentication data")
                return AuthResult(success=False, handled=True)
        parts = decoded.split(b"\0")
        if len(parts) != 3 or parts[0] not in (b"", USERNAME):
            return AuthResult(success=False, handled=False)
        return await self._authorize(server, parts[1], parts[2])

    async def auth_LOGIN(self, server, args):
        if len(args) == 1:
            username = await server.challenge_auth("Username:")
            if username is MISSING:
                return AuthResult(success=False)
        else:
            try:
                username = base64.b64decode(args[1], validate=True)
            except (ValueError, base64.binascii.Error):
                await server.push("501 5.5.2 Invalid authentication data")
                return AuthResult(success=False, handled=True)
        password = await server.challenge_auth("Password:")
        if password is MISSING:
            return AuthResult(success=False)
        return await self._authorize(server, username, password)

    async def handle_MAIL(self, server, session, envelope, address, mail_options):
        if not session.authenticated or not getattr(session, "mailer_key", None):
            return "530 5.7.0 Authentication required"
        if not address or "@" not in address:
            return "553 5.1.7 Invalid sender"
        envelope.mail_from = address
        envelope.mail_options.extend(mail_options)
        return "250 2.1.0 Sender accepted"

    async def handle_RCPT(self, server, session, envelope, address, rcpt_options):
        if not address or "@" not in address:
            return "553 5.1.3 Invalid recipient"
        if len(envelope.rcpt_tos) >= MAX_RECIPIENTS:
            return "452 4.5.3 Too many recipients"
        envelope.rcpt_tos.append(address)
        envelope.rcpt_options.extend(rcpt_options)
        return "250 2.1.5 Recipient accepted"

    async def handle_DATA(self, server, session, envelope):
        key = getattr(session, "mailer_key", None)
        if not session.authenticated or not key:
            return "530 5.7.0 Authentication required"
        raw = envelope.original_content
        if len(raw) > MAX_MESSAGE_BYTES:
            return "552 5.3.4 Message too large"
        try:
            payload = api_message(raw, envelope.mail_from, envelope.rcpt_tos)
        except (ValueError, UnicodeError, TypeError, LookupError):
            return "550 5.6.0 Unsupported or invalid message"
        peer = session.peer[0]
        fingerprint = hashlib.sha256(
            key.encode() + b"\0" + envelope.mail_from.encode() + b"\0"
            + b"\0".join(address.encode() for address in envelope.rcpt_tos)
            + b"\0" + raw
        ).hexdigest()
        status, response = await asyncio.to_thread(
            api_request, "/v1/emails", key, payload, peer, "smtp:" + fingerprint
        )
        if status in (200, 202) and response and response.get("data", {}).get("id"):
            return "250 2.0.0 Queued as " + response["data"]["id"]
        if status in (400, 403, 409, 422):
            return "550 5.7.1 Message rejected by Mailer"
        if status in (413,):
            return "552 5.3.4 Message too large"
        if status in (429,):
            return "452 4.5.3 Rate limit exceeded"
        if status in (401, 423):
            return "535 5.7.8 API key no longer authorized"
        return "451 4.3.0 Mailer temporarily unavailable"


def main():
    logging.basicConfig(level=logging.INFO)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(CERT, KEY)
    handler = MailerHandler()
    common = dict(
        hostname="0.0.0.0", server_hostname="smtp.mailer.crescentsphere.com",
        auth_required=True, data_size_limit=MAX_MESSAGE_BYTES,
        timeout=60, command_call_limit={"AUTH": 3},
    )
    # aiosmtpd detects STARTTLS via its internal protocol flag. On the implicit
    # TLS listener the socket is already wrapped before SMTP receives it.
    implicit = Controller(handler, port=465, ssl_context=context, auth_require_tls=False, **common)
    starttls = Controller(handler, port=587, tls_context=context, require_starttls=True, auth_require_tls=True, **common)
    implicit.start()
    starttls.start()
    logging.info("Mailer SMTP submission started on 465 and 587")
    try:
        import signal
        signal.pause()
    finally:
        starttls.stop()
        implicit.stop()


if __name__ == "__main__":
    main()
