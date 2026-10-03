-- Consent auditing (Section 10.1: registration validates the consent
-- version). Records which version of the terms/privacy notice each account
-- accepted and when. NULL for accounts created before consent tracking.

ALTER TABLE users
    ADD COLUMN consent_version TEXT,
    ADD COLUMN consented_at TIMESTAMPTZ;
