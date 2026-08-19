CREATE TABLE delivery_dead_letters (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email_id uuid REFERENCES emails(id) ON DELETE SET NULL,
    reason text NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX delivery_dead_letters_email_idx ON delivery_dead_letters(email_id, created_at DESC);
