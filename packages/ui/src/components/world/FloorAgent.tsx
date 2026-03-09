import { Agent } from '../../lib/types';
import { AgentActivity, VmState } from './worldTypes';
import AgentDesk from './AgentDesk';
import ThoughtBubble from './ThoughtBubble';
import { FLOOR } from './worldConstants';

interface FloorAgentProps {
    agent: Agent;
    activity: AgentActivity | null;
    vmState: VmState | null;
    deskPosition: [number, number, number];
    deskScale: number;
}

export default function FloorAgent({ agent, activity, vmState, deskPosition, deskScale }: FloorAgentProps) {
    const isWorking = activity?.status === 'WORKING';

    return (
        <group>
            {/* Desk is always rendered */}
            <AgentDesk
                position={deskPosition}
                scale={deskScale}
                agentName={agent.name}
                role={agent.role}
                status={agent.status}
                vmState={vmState}
                isOccupied={isWorking}
                isWorking={isWorking}
            />

            {/* Thought bubble above desk when working */}
            {isWorking && activity?.task && (
                <ThoughtBubble
                    position={[
                        deskPosition[0],
                        deskPosition[1] + FLOOR.DESK_Y + FLOOR.AGENT_BODY_HEIGHT + FLOOR.AGENT_HEAD_RADIUS * 2 + 0.3,
                        deskPosition[2] + FLOOR.DESK_DEPTH / 2 + 0.15,
                    ]}
                    text={activity.task}
                />
            )}
        </group>
    );
}
