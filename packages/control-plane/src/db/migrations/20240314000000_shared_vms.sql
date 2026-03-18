-- Shared VMs: department test, company test, and company production servers
-- that multiple agents can access based on role-based access control.

-- Expand vm_type to include shared VM types
ALTER TABLE vms DROP CONSTRAINT IF EXISTS vms_vm_type_check;
ALTER TABLE vms ADD CONSTRAINT vms_vm_type_check
    CHECK (vm_type IN ('desktop', 'sandbox', 'dept_test', 'company_test', 'company_prod'));

-- Ownership/scope table for shared VMs
CREATE TABLE shared_vms (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    -- DEPARTMENT scope = dept test VM; COMPANY scope = company test/prod server
    scope_type TEXT NOT NULL CHECK (scope_type IN ('DEPARTMENT', 'COMPANY')),
    company_id UUID NOT NULL REFERENCES companies(id),
    -- For department-scoped VMs, the manager who heads the department.
    -- NULL for company-scoped VMs.
    department_manager_id UUID REFERENCES agents(id),
    vm_purpose TEXT NOT NULL CHECK (vm_purpose IN ('dept_test', 'company_test', 'company_prod')),
    provisioned_by_agent_id UUID NOT NULL REFERENCES agents(id),
    -- Human-readable label (e.g. "Acme Engineering Staging")
    label TEXT,
    -- Default resource limits; can be expanded later
    resource_limits JSONB NOT NULL DEFAULT '{"vcpus": 2, "memory_mb": 2048, "disk_gb": 20}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- One VM per purpose per scope (company+dept for dept_test, company for company_test/prod)
    UNIQUE (company_id, department_manager_id, vm_purpose)
);

CREATE INDEX idx_shared_vms_company ON shared_vms(company_id);
CREATE INDEX idx_shared_vms_vm ON shared_vms(vm_id);
