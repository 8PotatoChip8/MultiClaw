-- Add CAPITAL_INJECTION to ledger entry types
ALTER TABLE ledger_entries DROP CONSTRAINT ledger_entries_type_check;
ALTER TABLE ledger_entries ADD CONSTRAINT ledger_entries_type_check
    CHECK (type IN ('EXPENSE','REVENUE','INTERNAL_TRANSFER','CAPITAL_INJECTION'));
