"""Isolated API/worker regression suite. Requires Docker and the three usable-test images.

Build: docker build --target api -t mailer-usable-api:local backend
       docker build --target worker -t mailer-usable-worker:local backend
       docker build --build-arg NGINX_CONFIG=nginx.production.conf -t mailer-usable-frontend:local frontend
Run:   python backend/deploy/tests/integration.py
Never reads .env, starts cloudflared, or sends provider email. --keep supports local UI QA.
"""
import argparse
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[3]
PASSWORD = 'Integration-only-password-123'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--keep', action='store_true')
    options = parser.parse_args()
    project = 'mailer-integration-' + uuid.uuid4().hex[:10]
    temporary = tempfile.TemporaryDirectory(prefix='mailer-integration-')
    directory = Path(temporary.name)
    values = {key: secrets.token_hex(32) for key in ['POSTGRES_PASSWORD', 'NATS_PASSWORD', 'EVENT_INGEST_TOKEN', 'WEBHOOK_SIGNING_MASTER_KEY']}
    values.update(APP_ENV='development', DOMAIN_PROVIDER='disabled', OBJECT_STORAGE_PROVIDER='disabled',
                  ACCOUNT_EMAIL_FROM='account@integration.invalid', SES_CONFIGURATION_SET='',
                  TURNSTILE_SITE_KEY='1x00000000000000000000AA',
                  TURNSTILE_SECRET_KEY='1x0000000000000000000000000000000AA',
                  SES_EVENTS_QUEUE_URL='', SES_EVENTS_TOPIC_ARN='', CLOUDFLARE_TUNNEL_TOKEN='unused',
                  API_AWS_ACCESS_KEY_ID='unused', API_AWS_SECRET_ACCESS_KEY='unused',
                  WORKER_AWS_ACCESS_KEY_ID='unused', WORKER_AWS_SECRET_ACCESS_KEY='unused', FRONTEND_PORT='0',
                  API_KEY_RATE_LIMIT_PER_MINUTE='1000', CLIENT_IP_RATE_LIMIT_PER_MINUTE='1000')
    envfile = directory / 'test.env'
    envfile.write_text('\n'.join(f'{k}={v}' for k, v in values.items()) + '\n')
    override = directory / 'compose.json'
    override.write_text(json.dumps({'services': {
        'api': {'image': 'mailer-usable-api:local', 'environment': {'AWS_EC2_METADATA_DISABLED': 'true'}},
        'frontend': {'image': 'mailer-usable-frontend:local'},
        'worker': {'image': 'mailer-usable-worker:local', 'environment': {
            'AWS_EC2_METADATA_DISABLED': 'true', 'ACCOUNT_EMAIL_FROM': '',
            'AWS_ENDPOINT_URL_SESV2': 'http://127.0.0.1:9'}}
    }}))
    environment = dict(os.environ, **values)
    compose = ['docker', 'compose', '--project-name', project, '--env-file', str(envfile),
               '-f', str(ROOT / 'docker-compose.production.yml'), '-f', str(override)]

    def run(args, data=None):
        result = subprocess.run(compose + args, input=data, text=True, capture_output=True, env=environment)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return result.stdout

    def sql(query):
        return run(['exec', '-T', 'postgres', 'psql', '-U', 'mailer', '-d', 'mailer', '-At', '-v', 'ON_ERROR_STOP=1'], query).strip()

    def request(method, path, body=None, cookie=None, key=None, idem=None, internal=False):
        url = 'http://127.0.0.1:8080' if internal else 'http://127.0.0.1:8081/api'
        args = ['exec', '-T', 'api', 'curl', '-sS', '-i', '-X', method, url + path, '-H', 'Content-Type: application/json']
        for name, value in [('Cookie', cookie), ('Authorization', 'Bearer ' + key if key else None), ('Idempotency-Key', idem)]:
            if value:
                args += ['-H', f'{name}: {value}']
        if body is not None:
            args += ['--data-binary', '@-']
        raw = run(args, json.dumps(body) if body is not None else None)
        headers, payload = raw.split('\n\n', 1)
        status = int(headers.splitlines()[0].split()[1])
        cookies = [line.split(': ', 1)[1].split(';')[0] for line in headers.splitlines() if line.lower().startswith('set-cookie:')]
        return status, json.loads(payload) if payload.strip().startswith('{') else payload, cookies[0] if cookies else None

    def expect(status, response):
        assert response[0] == status, f'Expected HTTP {status}, got {response[0]}: {response[1]}'
        return response[1]

    def signup(email):
        response = request('POST', '/v1/auth/signup', {'email': email, 'password': PASSWORD, 'first_name': 'Integration',
                           'last_name': 'Owner', 'turnstile_token': 'XXXX.DUMMY.TOKEN.XXXX'})
        assert response[0] in (200, 201), (response[0], response[1])
        assert response[1]['data']['verificationRequired'] and response[2] is None
        expect(403, request('POST', '/v1/auth/login', {'email': email, 'password': PASSWORD}))
        first_token = re.search(r'token=([A-Za-z0-9_-]+)', sql(f"SELECT body FROM account_emails WHERE recipient='{email}' ORDER BY updated_at DESC LIMIT 1;")).group(1)
        expect(200, request('POST', '/v1/auth/email-verification/resend', {'email': email}))
        token = re.search(r'token=([A-Za-z0-9_-]+)', sql(f"SELECT body FROM account_emails WHERE recipient='{email}' ORDER BY updated_at DESC LIMIT 1;")).group(1)
        assert token != first_token
        expect(400, request('POST', '/v1/auth/email-verification/complete', {'token': first_token}))
        verified = request('POST', '/v1/auth/email-verification/complete', {'token': token})
        return expect(200, verified)['data'], verified[2]

    def send(body, key, idem):
        return request('POST', '/v1/emails', body, key=key, idem=idem)

    kept = False
    try:
        run(['up', '-d', '--no-build', '--wait', '--wait-timeout', '120', 'api', 'frontend'])
        session, cookie = signup('owner@integration.invalid')
        other, other_cookie = signup('other@integration.invalid')
        workspace = session['workspace']['id']
        sql(f"INSERT INTO domains (workspace_id,name,status,provider_status) VALUES ('{workspace}','integration.invalid','verified','verified');")
        keys = {}
        payload = expect(200, request('POST', '/v1/api-keys', {'name': 'test', 'environment': 'test',
            'scopes': ['emails:send', 'emails:read', 'domains:read', 'webhooks:manage', 'workspace:read', 'suppressions:manage']}, cookie=cookie))
        keys['test'] = payload['data']
        expect(403, request('POST', '/v1/api-keys', {'name': 'production', 'environment': 'production', 'scopes': ['emails:send']}, cookie=cookie))
        sql(f"UPDATE workspaces SET production_enabled=true WHERE id='{workspace}';")
        for mode in ['production']:
            payload = expect(200, request('POST', '/v1/api-keys', {'name': mode, 'environment': mode,
                'scopes': ['emails:send', 'emails:read', 'domains:read', 'webhooks:manage', 'workspace:read', 'suppressions:manage']}, cookie=cookie))
            keys[mode] = payload['data']
        test_key, live_key = keys['test']['secret'], keys['production']['secret']
        body = {'from': 'sender@integration.invalid', 'to': ['recipient@integration.invalid'], 'subject': 'Integration test', 'text': 'Not sent externally', 'metadata': {'orderId':'123'}}
        read_key = expect(200, request('POST', '/v1/api-keys', {'name': 'read only', 'environment': 'test', 'scopes': ['emails:read']}, cookie=cookie))['data']['secret']
        expect(200, request('GET', '/v1/emails', key=read_key))
        # The API deliberately uses the same 401 response for invalid and under-scoped keys.
        expect(401, send(body, read_key, 'read-only-denied'))
        expect(401, request('GET', '/v1/webhooks', key=read_key))
        test = expect(202, send(body, test_key, 'shared'))['data']['id']
        assert expect(200, send(body, test_key, 'shared'))['data']['id'] == test
        assert expect(200, send(dict(body, environment='test'), test_key, 'shared'))['data']['id'] == test
        live = expect(202, send(body, live_key, 'shared'))['data']['id']
        assert live != test
        expect(409, send(dict(body, subject='Changed'), test_key, 'shared'))
        expect(400, send(dict(body, environment='production'), test_key, 'mismatch'))
        expect(404, request('GET', f'/v1/emails/{live}', key=test_key))
        expect(404, request('GET', f'/v1/emails/{test}', cookie=other_cookie))
        assert expect(200, request('GET', '/v1/emails', cookie=other_cookie))['data'] == []
        expect(404, request('DELETE', '/v1/api-keys/' + keys['test']['key']['id'], cookie=other_cookie))
        expect(401, request('GET','/v1/emails',cookie=cookie,key='invalid'))
        for path in ['/v1/domains', '/v1/webhooks', '/v1/workspace']:
            expect(200, request('GET', path, key=test_key))
        assert expect(200,request('GET','/v1/emails?limit=1',cookie=cookie))['hasMore']
        print('PASS: verified public signup, production approval, sessions, tenant/scope/environment isolation, retrieval and idempotency', flush=True)

        for url in ['https://127.0.0.1/', 'https://[::1]/', 'https://[::ffff:127.0.0.1]/', 'http://hooks.example.com/']:
            expect(400, request('POST', '/v1/webhooks', {'url':url,'environment':'test','subscriptions':['email.delivery']}, cookie=cookie))
        endpoints = {}
        for mode in ['test','production']:
            # Reserved .invalid name can never deliver an external webhook.
            endpoints[mode] = expect(200, request('POST','/v1/webhooks',{'url':'https://hooks.integration.invalid/events','environment':mode,'subscriptions':['email.delivery','email.bounce','email.complaint']},cookie=cookie))['data']['endpoint']['id']
        expect(200,request('PATCH','/v1/webhooks/'+endpoints['test'],{'enabled':False},cookie=cookie))
        expect(200,request('PATCH','/v1/webhooks/'+endpoints['test'],{'enabled':True},cookie=cookie))
        old = keys['test']['key']['id']
        count = sql('SELECT count(*) FROM api_keys;')
        sql("CREATE FUNCTION deny_revoke() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test fault'; END $$; CREATE TRIGGER deny_revoke BEFORE UPDATE OF revoked_at ON api_keys FOR EACH ROW EXECUTE FUNCTION deny_revoke();")
        expect(503,request('POST',f'/v1/api-keys/{old}/rotate',{},cookie=cookie))
        assert sql('SELECT count(*) FROM api_keys;') == count
        assert sql(f"SELECT revoked_at IS NULL FROM api_keys WHERE id='{old}';") == 't'
        sql('DROP TRIGGER deny_revoke ON api_keys; DROP FUNCTION deny_revoke();')
        test_key = expect(200,request('POST',f'/v1/api-keys/{old}/rotate',{},cookie=cookie))['data']['secret']
        expect(401,send(body,keys['test']['secret'],'revoked'))
        expect(400,send(dict(body,to=[f'r{i}@integration.invalid' for i in range(51)]),test_key,'too-many'))
        expect(400,send(dict(body,headers={'X-Custom':'ignored-before'}),test_key,'headers'))
        suppression = expect(200,request('POST','/v1/suppressions',{'address':'blocked@integration.invalid'},cookie=cookie))['data']['id']
        expect(422,send(dict(body,to=['blocked@integration.invalid']),test_key,'suppressed'))
        expect(404,request('DELETE',f'/v1/suppressions/{suppression}',cookie=other_cookie))
        expect(200,request('DELETE',f'/v1/suppressions/{suppression}',cookie=cookie))
        print('PASS: safe webhook destinations, atomic rotation with DB fault, limits and suppressions', flush=True)

        sql(f"UPDATE emails SET status='sent',provider_message_id='integration-provider-id',sent_at=now() WHERE id='{live}';")
        event={'eventId':'integration-bounce','messageId':'integration-provider-id','eventType':'bounce','occurredAt':'2026-08-31T00:01:00Z','recipients':['recipient@integration.invalid'],'bounceType':'Permanent','details':{}}
        expect(401,request('POST','/internal/v1/ses/events',event,internal=True,key='invalid'))
        expect(200,request('POST','/internal/v1/ses/events',event,internal=True,key=values['EVENT_INGEST_TOKEN']))
        expect(200,request('POST','/internal/v1/ses/events',event,internal=True,key=values['EVENT_INGEST_TOKEN']))
        assert sql(f"SELECT count(*) FROM delivery_events WHERE email_id='{live}';") == '1'
        payload = expect(200,request('GET',f'/v1/emails/{live}',key=live_key))['data']
        assert payload['status']=='bounced' and payload['events'][0]['data']['emailId']==live
        # Clear this intentionally suppressed recipient so the pending test can proceed.
        sql("DELETE FROM suppressions WHERE address='recipient@integration.invalid';")
        simulation = expect(202,send(dict(body, **{'from':'sender@sandbox.mailer.invalid','to':['demo@example.com']}),test_key,'sandbox'))['data']['id']
        bounce = expect(202,send(dict(body, **{'from':'sender@sandbox.mailer.invalid','to':['bounce@simulator.mailer.invalid']}),test_key,'simulated-bounce'))['data']['id']
        assert sql("SELECT count(*) FROM emails WHERE environment='production' AND status IN ('queued','processing');")=='0'
        run(['up','-d','--no-build','worker'])
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            if sql(f"SELECT status FROM emails WHERE id='{simulation}';")=='delivered' and sql(f"SELECT status FROM emails WHERE id='{bounce}';")=='bounced': break
            time.sleep(1)
        else: raise AssertionError('Worker did not process simulations: '+run(['logs','--tail','20','worker']))
        simulated = expect(200,request('GET',f'/v1/emails/{simulation}',key=test_key))['data']
        assert simulated['content']['text']==body['text'] and simulated['events'][0]['data']['environment']=='test'
        deadline=time.monotonic()+15
        while time.monotonic()<deadline:
            if sql(f"SELECT count(*) FROM webhook_deliveries d JOIN delivery_events e ON e.id=d.event_id WHERE e.email_id='{simulation}';")=='1': break
            time.sleep(1)
        assert sql(f"SELECT endpoint_id FROM webhook_deliveries d JOIN delivery_events e ON e.id=d.event_id WHERE e.email_id='{simulation}';")==endpoints['test']
        sql('UPDATE webhook_endpoints SET enabled=false;')
        expect(422,send(dict(body,to=['bounce@simulator.mailer.invalid']),test_key,'repeat-bounce'))
        print('PASS: real NATS worker, delivery/bounce simulation, event correlation, deduplication and webhook environment routing',flush=True)

        expect(200,request('POST','/v1/auth/password-reset/request',{'email':'owner@integration.invalid'}))
        token = re.search(r'token=([A-Za-z0-9_-]+)',sql("SELECT body FROM account_emails ORDER BY updated_at DESC LIMIT 1;")).group(1)
        expect(200,request('POST','/v1/auth/password-reset/complete',{'token':token,'password':PASSWORD+'-changed'}))
        expect(400,request('POST','/v1/auth/password-reset/complete',{'token':token,'password':PASSWORD}))
        expect(401,request('GET','/v1/auth/session',cookie=cookie))
        expect(200,request('POST','/v1/auth/login',{'email':'owner@integration.invalid','password':PASSWORD+'-changed'}))
        print('PASS: durable reset queue, one-time token, password change and session revocation',flush=True)
        if options.keep:
            port=run(['port','api','8081']).strip()
            state=ROOT/'.work'/'integration-state.json';state.parent.mkdir(exist_ok=True)
            state.write_text(json.dumps({'compose':compose,'environment':values,'directory':str(directory),'url':'http://'+port,'project':project}))
            # Retain only this isolated stack for browser testing; caller must clean it up.
            temporary._finalizer.detach()
            kept=True
            print('UI test stack ready at http://'+port,flush=True)
        else:
            # Validate strict production startup without running any provider worker.
            run(['stop', 'worker'])
            environment.update(APP_ENV='production', DOMAIN_PROVIDER='ses', OBJECT_STORAGE_PROVIDER='r2',
                SES_CONFIGURATION_SET='unused', TURNSTILE_SITE_KEY='unused-site-key', TURNSTILE_SECRET_KEY='unused-secret-key-that-is-long-enough',
                SES_EVENTS_QUEUE_URL='https://sqs.ap-southeast-1.amazonaws.com/000000000000/unused',
                SES_EVENTS_TOPIC_ARN='arn:aws:sns:ap-southeast-1:000000000000:unused',
                OBJECT_STORAGE_ENDPOINT='http://127.0.0.1:9', OBJECT_STORAGE_BUCKET='unused',
                OBJECT_STORAGE_ACCESS_KEY_ID='unused', OBJECT_STORAGE_SECRET_ACCESS_KEY='unused')
            run(['up', '-d', '--no-build', '--wait', '--wait-timeout', '120', 'api', 'frontend'])
            expect(200, request('GET', '/readyz'))
            expect(404, request('POST', '/internal/v1/ses/events', {}))
            print('PASS: strict production API startup, migrations, authenticated NATS and private-route blocking', flush=True)
    finally:
        if not kept:
            run(['down','--volumes','--remove-orphans'])
            temporary.cleanup()

if __name__=='__main__': main()
