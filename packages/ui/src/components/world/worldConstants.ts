// Colors matching the app's CSS variable palette
export const COLORS = {
    // Building types
    INTERNAL: '#8b5cf6',       // --accent (purple)
    INTERNAL_DARK: '#6d3ecf',
    EXTERNAL: '#3b82f6',       // --primary (blue)
    EXTERNAL_DARK: '#2563c0',

    // Ground and environment
    GROUND: '#0d1117',
    GRID: '#1a2332',
    SKY: '#0a0e1a',            // --bg

    // Agent roles
    CEO: '#3b82f6',            // blue
    MANAGER: '#10b981',        // green
    WORKER: '#7b8ba8',         // muted

    // VM monitor states
    DESKTOP_ON: '#10b981',     // green glow
    SANDBOX_ON: '#3b82f6',     // blue glow
    MONITOR_OFF: '#1a1a2e',    // dark

    // Status
    QUARANTINED: '#ef4444',    // red
    HOVER_GLOW: '#f59e0b',     // warm glow on hover

    // Furniture
    DESK: '#2a3444',
    CHAIR: '#3a4454',
    WALL: '#1e293b',
    FLOOR: '#151d2b',
    BREAK_ROOM: '#1a2636',

    // Text
    LABEL: '#e8ecf4',
    LABEL_MUTED: '#7b8ba8',
};

// Building sizing
export const BUILDING = {
    MIN_WIDTH: 1.2,
    MAX_WIDTH: 3.5,
    MIN_HEIGHT: 2,
    MAX_HEIGHT: 10,
    MIN_DEPTH: 1.2,
    MAX_DEPTH: 3.5,
    SPACING: 5,
    WINDOW_ROWS: 4,
};

// Floor view
export const FLOOR = {
    DESK_WIDTH: 1.0,
    DESK_DEPTH: 0.5,
    DESK_HEIGHT: 0.05,
    DESK_Y: 0.4,
    MONITOR_WIDTH: 0.25,
    MONITOR_HEIGHT: 0.18,
    MONITOR_DEPTH: 0.02,
    CHAIR_RADIUS: 0.12,
    CHAIR_HEIGHT: 0.35,
    AGENT_HEAD_RADIUS: 0.1,
    AGENT_BODY_HEIGHT: 0.3,
    AGENT_BODY_WIDTH: 0.2,
    DESK_SPACING_X: 1.8,
    DESK_SPACING_Z: 1.5,
    CEO_DESK_SCALE: 1.4,
    MANAGER_DESK_SCALE: 1.15,
    ROOM_HEIGHT: 2.5,
    WALL_THICKNESS: 0.08,
};
