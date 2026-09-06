ALTER TABLE delivery_operator_controls
    ADD COLUMN IF NOT EXISTS default_provider text
        CHECK (default_provider IS NULL OR default_provider IN ('ses', 'smtp'));

COMMENT ON COLUMN delivery_operator_controls.default_provider IS
    'Runtime override for DELIVERY_PROVIDER. NULL follows the environment setting.';
