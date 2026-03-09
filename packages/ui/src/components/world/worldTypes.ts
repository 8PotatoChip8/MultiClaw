import { Agent, AgentActivity as BaseAgentActivity, Company } from '../../lib/types';

export type ViewMode = 'city' | 'floor';

export interface AgentActivity extends BaseAgentActivity {
    agent_id: string;
}

export interface VmState {
    desktop: 'RUNNING' | 'STOPPED' | 'UNKNOWN';
    sandbox: 'RUNNING' | 'STOPPED' | 'UNKNOWN';
}

export interface WorldSnapshot {
    companies: Company[];
    agents: Agent[];
    balances: Record<string, Record<string, { revenue: number; expenses: number; capital: number; net: number }>>;
    activities: Record<string, AgentActivity>;
    vm_states: Record<string, VmState>;
}

export interface BuildingData {
    company: Company;
    agentCount: number;
    netBalance: number;
    totalCapital: number;
    position: [number, number, number];
    height: number;
    width: number;
    depth: number;
    color: string;
}

export interface FloorAgentData {
    agent: Agent;
    activity: AgentActivity | null;
    vmState: VmState | null;
    role: 'CEO' | 'MANAGER' | 'WORKER';
    deskPosition: [number, number, number];
}
