-- Add vm_type to vms table (desktop or sandbox)
ALTER TABLE vms ADD COLUMN vm_type TEXT NOT NULL DEFAULT 'desktop'
    CHECK (vm_type IN ('desktop', 'sandbox'));

-- Add sandbox_vm_id to agents (desktop stays in vm_id)
ALTER TABLE agents ADD COLUMN sandbox_vm_id UUID REFERENCES vms(id);
