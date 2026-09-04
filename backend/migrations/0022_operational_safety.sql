CREATE TABLE service_heartbeats (
    component text PRIMARY KEY,
    instance_id uuid NOT NULL,
    details jsonb NOT NULL DEFAULT '{}'::jsonb,
    updated_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE emails
    ADD COLUMN content_cleanup_attempted_at timestamptz;

CREATE INDEX emails_content_cleanup_due_idx
    ON emails(COALESCE(completed_at,sent_at,accepted_at), content_cleanup_attempted_at)
    WHERE raw_object_key IS NOT NULL AND content_deleted_at IS NULL;

CREATE FUNCTION emit_local_delivery_failure() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    event_id uuid := gen_random_uuid();
    inserted_event uuid;
BEGIN
    IF NEW.status = 'failed'
       AND OLD.status IN ('queued','processing')
       AND NEW.last_error IS NOT NULL THEN
        UPDATE email_recipients
        SET status='failed'
        WHERE email_id=NEW.id AND status IN ('pending','sent');

        INSERT INTO delivery_events(id,email_id,provider_event_id,event_type,payload,occurred_at)
        VALUES(
            event_id,
            NEW.id,
            'local:' || NEW.id::text || ':failed',
            'reject',
            jsonb_build_object(
                'emailId',NEW.id,
                'environment',NEW.environment,
                'metadata',NEW.metadata,
                'eventType','reject',
                'recipients',jsonb_build_array(),
                'details',jsonb_build_object('source','worker','reason',NEW.last_error)
            ),
            now()
        )
        ON CONFLICT(provider_event_id) DO NOTHING
        RETURNING id INTO inserted_event;

        IF inserted_event IS NOT NULL THEN
            INSERT INTO outbox_events(aggregate_type,aggregate_id,event_type,payload)
            VALUES(
                'delivery_event',
                inserted_event,
                'email.reject',
                jsonb_build_object(
                    'deliveryEventId',inserted_event,
                    'emailId',NEW.id,
                    'workspaceId',NEW.workspace_id
                )
            );
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER emails_local_failure_event
AFTER UPDATE OF status ON emails
FOR EACH ROW EXECUTE FUNCTION emit_local_delivery_failure();
