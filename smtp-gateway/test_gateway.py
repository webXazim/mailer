from email.message import EmailMessage
import os
from pathlib import Path
import smtplib
import socket
import ssl
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
os.environ.setdefault("SMTP_GATEWAY_SHARED_SECRET", "test-only-shared-secret")
import gateway


class MessageTests(unittest.TestCase):
    def message(self):
        message = EmailMessage()
        message["From"] = "Team <team@example.com>"
        message["To"] = "Person <person@example.net>"
        message["Cc"] = "Other <other@example.net>"
        message["Subject"] = "Status"
        message.set_content("Hello")
        message.add_alternative("<p>Hello</p>", subtype="html")
        return message

    def test_mime_and_envelope_become_api_request(self):
        message = self.message()
        result = gateway.api_message(message.as_bytes(), "team@example.com", [
            "person@example.net", "other@example.net", "hidden@example.net"
        ])
        self.assertEqual(result["to"], ["person@example.net"])
        self.assertEqual(result["cc"], ["other@example.net"])
        self.assertEqual(result["bcc"], ["hidden@example.net"])
        self.assertIn("Hello", result["text"])
        self.assertIn("<p>Hello</p>", result["html"])

    def test_sender_spoof_is_rejected(self):
        with self.assertRaises(ValueError):
            gateway.api_message(self.message().as_bytes(), "spoof@example.com", ["person@example.net"])

    def test_attachment_is_preserved(self):
        message = self.message()
        message.add_attachment(b"report", maintype="application", subtype="pdf", filename="report.pdf")
        result = gateway.api_message(message.as_bytes(), "team@example.com", ["person@example.net"])
        self.assertEqual(result["attachments"][0]["filename"], "report.pdf")
        self.assertEqual(result["attachments"][0]["content"], "cmVwb3J0")


class SubmissionTests(unittest.IsolatedAsyncioTestCase):
    async def test_no_auth_relay(self):
        handler = gateway.MailerHandler()
        session = types.SimpleNamespace(authenticated=False)
        envelope = types.SimpleNamespace(mail_from="", mail_options=[])
        status = await handler.handle_MAIL(None, session, envelope, "team@example.com", [])
        self.assertTrue(status.startswith("530"))

    async def test_queued_only_after_api_accepts(self):
        handler = gateway.MailerHandler()
        session = types.SimpleNamespace(authenticated=True, mailer_key="cs_live_test", peer=("127.0.0.1", 2345))
        envelope = types.SimpleNamespace(
            mail_from="team@example.com", rcpt_tos=["person@example.net"],
            original_content=MessageTests().message().as_bytes(),
        )
        with patch.object(gateway, "api_request", return_value=(202, {"data": {"id": "abc"}})):
            self.assertEqual(await handler.handle_DATA(None, session, envelope), "250 2.0.0 Queued as abc")
        with patch.object(gateway, "api_request", return_value=(503, None)):
            self.assertTrue((await handler.handle_DATA(None, session, envelope)).startswith("451"))


class SmtpProtocolTests(unittest.TestCase):
    def test_implicit_tls_and_starttls_require_valid_key(self):
        try:
            import trustme
        except ImportError:
            self.skipTest("trustme is needed for the TLS integration test")
        authority = trustme.CA()
        server_cert = authority.issue_cert("localhost")
        with tempfile.TemporaryDirectory() as directory:
            server_cert.cert_chain_pems[0].write_to_path(Path(directory) / "server.pem")
            server_cert.private_key_pem.write_to_path(Path(directory) / "server.key")
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.load_cert_chain(str(Path(directory) / "server.pem"), str(Path(directory) / "server.key"))
            client_context = authority.cert_pem  # Build a verifying client context below.
            verifying = ssl.create_default_context()
            verifying.load_verify_locations(cadata=client_context.bytes().decode())

            def free_port():
                with socket.socket() as sock:
                    sock.bind(("127.0.0.1", 0))
                    return sock.getsockname()[1]

            def fake_api(path, key, payload=None, client_ip=None, idempotency=None):
                if key != "cs_live_valid":
                    return 401, None
                if path.endswith("/verify"):
                    return 200, {"environment": "production"}
                self.assertEqual(payload["subject"], "Status")
                self.assertEqual(payload["to"], ["person@example.net"])
                return 202, {"data": {"id": "queued-123"}}

            implicit_port, starttls_port = free_port(), free_port()
            handler = gateway.MailerHandler()
            implicit = gateway.Controller(
                handler, hostname="127.0.0.1", port=implicit_port,
                ssl_context=context, auth_required=True, auth_require_tls=False,
            )
            starttls = gateway.Controller(
                handler, hostname="127.0.0.1", port=starttls_port,
                tls_context=context, require_starttls=True,
                auth_required=True, auth_require_tls=True,
            )
            with patch.object(gateway, "api_request", side_effect=fake_api):
                implicit.start()
                starttls.start()
                try:
                    with smtplib.SMTP_SSL("localhost", implicit_port, context=verifying) as client:
                        with self.assertRaises(smtplib.SMTPSenderRefused):
                            client.send_message(MessageTests().message())
                        with self.assertRaises(smtplib.SMTPAuthenticationError):
                            client.login("mailer", "cs_live_invalid")
                        client.login("mailer", "cs_live_valid")
                        self.assertEqual(client.send_message(MessageTests().message()), {})
                    with smtplib.SMTP("localhost", starttls_port) as client:
                        client.ehlo()
                        self.assertFalse(client.has_extn("auth"))
                        client.starttls(context=verifying)
                        client.ehlo()
                        client.login("mailer", "cs_live_valid")
                        self.assertEqual(client.send_message(MessageTests().message()), {})
                finally:
                    starttls.stop()
                    implicit.stop()


if __name__ == "__main__":
    unittest.main()
