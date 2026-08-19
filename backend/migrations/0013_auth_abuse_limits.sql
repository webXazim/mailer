CREATE TABLE auth_rate_limits (
    bucket_key text NOT NULL,
    bucket_start timestamptz NOT NULL,
    attempt_count integer NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_key, bucket_start)
);
CREATE INDEX auth_rate_limits_cleanup_idx ON auth_rate_limits(bucket_start);
