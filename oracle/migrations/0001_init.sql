-- Hivemind oracle — initial schema.
--
-- Fully relational for everything queryable. The plan body and a few opaque
-- payloads stay JSON because they are never queried by field, only ever read
-- whole or matched by hash. See oracle/README.md → Persistence for the
-- design rationale.

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

-- ─── Intents ─────────────────────────────────────────────────

CREATE TABLE intents (
    id              TEXT PRIMARY KEY,
    received_at     TEXT NOT NULL,
    source_file     TEXT,
    georeferenced   INTEGER NOT NULL CHECK (georeferenced IN (0, 1)),
    constraints     TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE intent_regions (
    intent_id       TEXT NOT NULL REFERENCES intents(id) ON DELETE CASCADE,
    id              TEXT NOT NULL,
    name            TEXT NOT NULL,
    area_m2         REAL NOT NULL,
    face_count      INTEGER NOT NULL,
    paint_spec      TEXT,
    faces           TEXT NOT NULL,
    PRIMARY KEY (intent_id, id)
);

-- ─── Plans ───────────────────────────────────────────────────

CREATE TABLE plans (
    id                              TEXT PRIMARY KEY,
    intent_id                       TEXT NOT NULL REFERENCES intents(id),
    status                          TEXT NOT NULL CHECK (status IN (
                                        'Draft', 'Proposed', 'Approved',
                                        'Executing', 'Paused',
                                        'Aborted', 'Complete', 'Failed'
                                    )),
    created_at                      TEXT NOT NULL,
    proposed_at                     TEXT,
    approved_at                     TEXT,
    approved_by                     TEXT,
    started_at                      TEXT,
    completed_at                    TEXT,

    body_hash                       TEXT NOT NULL,
    body                            TEXT NOT NULL,
    fleet_snapshot                  TEXT NOT NULL,

    coverage_total_area_m2          REAL NOT NULL,
    coverage_overlap_pct            REAL NOT NULL,
    schedule_total_duration_s       INTEGER NOT NULL,
    schedule_peak_concurrent_drones INTEGER NOT NULL,
    resources_paint_ml              REAL NOT NULL,
    resources_battery_cycles        INTEGER NOT NULL,
    resources_total_flight_time_s   INTEGER NOT NULL
);
CREATE INDEX plans_status_idx     ON plans(status);
CREATE INDEX plans_created_at_idx ON plans(created_at DESC);
CREATE INDEX plans_intent_idx     ON plans(intent_id);

CREATE TABLE plan_warnings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    severity        TEXT NOT NULL CHECK (severity IN ('info', 'warn', 'critical')),
    code            TEXT NOT NULL,
    message         TEXT NOT NULL,
    context         TEXT
);
CREATE INDEX plan_warnings_plan_idx ON plan_warnings(plan_id);

CREATE TABLE plan_errors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    code            TEXT NOT NULL,
    message         TEXT NOT NULL,
    context         TEXT
);
CREATE INDEX plan_errors_plan_idx ON plan_errors(plan_id);

-- ─── Drones (fleet roster) ───────────────────────────────────
-- Long-lived per-drone state. Updated whenever telemetry arrives.
-- Defined before sorties because sorties.drone_id references it.

CREATE TABLE drones (
    id                          TEXT PRIMARY KEY,
    first_seen_at               TEXT NOT NULL,
    last_seen_at                TEXT NOT NULL,
    legion_version              TEXT,
    capabilities                TEXT,
    last_known_battery_pct      REAL,
    last_known_paint_ml         REAL,
    last_known_position_lat     REAL,
    last_known_position_lon     REAL,
    last_known_position_alt_m   REAL,
    last_known_drone_phase      TEXT CHECK (last_known_drone_phase IN (
                                    'Idle', 'Armed', 'InAir',
                                    'ExecutingStep', 'Holding', 'Landing'
                                )),
    is_stale                    INTEGER NOT NULL DEFAULT 0 CHECK (is_stale IN (0, 1))
);
CREATE INDEX drones_last_seen_idx ON drones(last_seen_at DESC);

-- ─── Sorties + Steps ─────────────────────────────────────────

CREATE TABLE sorties (
    id                      TEXT PRIMARY KEY,
    plan_id                 TEXT NOT NULL REFERENCES plans(id),
    drone_id                TEXT NOT NULL REFERENCES drones(id),
    sortie_index            INTEGER NOT NULL,
    status                  TEXT NOT NULL CHECK (status IN (
                                'Pending', 'Uploaded', 'Executing',
                                'Complete', 'Failed', 'Aborted'
                            )),
    paint_volume_ml         REAL NOT NULL,
    expected_duration_s     INTEGER NOT NULL,
    uploaded_at             TEXT,
    started_at              TEXT,
    ended_at                TEXT,
    failure_reason          TEXT,
    UNIQUE (plan_id, sortie_index)
);
CREATE INDEX sorties_plan_idx     ON sorties(plan_id);
CREATE INDEX sorties_drone_idx    ON sorties(drone_id);
CREATE INDEX sorties_status_idx   ON sorties(status);

CREATE TABLE sortie_steps (
    sortie_id                       TEXT NOT NULL REFERENCES sorties(id) ON DELETE CASCADE,
    step_index                      INTEGER NOT NULL,
    step_type                       TEXT NOT NULL CHECK (step_type IN (
                                        'Takeoff', 'Transit', 'SprayPass',
                                        'RefillApproach', 'RefillWait',
                                        'ReturnToBase', 'Land'
                                    )),
    waypoint_lat                    REAL NOT NULL,
    waypoint_lon                    REAL NOT NULL,
    waypoint_alt_m                  REAL NOT NULL,
    waypoint_yaw_deg                REAL,
    speed_m_s                       REAL NOT NULL,
    spray                           INTEGER NOT NULL CHECK (spray IN (0, 1)),
    radio_loss_behaviour            TEXT NOT NULL CHECK (radio_loss_behaviour IN (
                                        'Continue', 'HoldThenRtl', 'RtlImmediately'
                                    )),
    radio_loss_silent_timeout_s     REAL NOT NULL,
    radio_loss_hold_then_rtl_after_s REAL,
    expected_duration_s             INTEGER NOT NULL,
    path                            TEXT,
    PRIMARY KEY (sortie_id, step_index)
);

CREATE TABLE step_progress (
    sortie_id               TEXT NOT NULL,
    step_index              INTEGER NOT NULL,
    state                   TEXT NOT NULL CHECK (state IN (
                                'Gating', 'Running', 'Complete',
                                'Failed', 'Held', 'Aborted'
                            )),
    gate_decision           TEXT CHECK (gate_decision IN (
                                'AutoProceed', 'OperatorRequired',
                                'FleetConflict', 'AbortSortie'
                            )),
    gate_reason             TEXT,
    gated_at                TEXT,
    started_at              TEXT,
    completed_at            TEXT,

    position_lat            REAL,
    position_lon            REAL,
    position_alt_m          REAL,
    battery_pct             REAL,
    paint_remaining_ml      REAL,
    duration_s              REAL,
    failure_reason          TEXT,

    PRIMARY KEY (sortie_id, step_index),
    FOREIGN KEY (sortie_id, step_index) REFERENCES sortie_steps(sortie_id, step_index)
);
CREATE INDEX step_progress_state_idx ON step_progress(state);

-- ─── Amendments ──────────────────────────────────────────────

CREATE TABLE amendments (
    id                  TEXT PRIMARY KEY,
    plan_id             TEXT NOT NULL REFERENCES plans(id),
    kind                TEXT NOT NULL,
    requires_approval   INTEGER NOT NULL CHECK (requires_approval IN (0, 1)),
    applied_at          TEXT NOT NULL,
    operator            TEXT,
    body                TEXT NOT NULL
);
CREATE INDEX amendments_plan_idx ON amendments(plan_id);

-- ─── Audit log ───────────────────────────────────────────────

CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              TEXT NOT NULL,
    actor           TEXT NOT NULL,
    event           TEXT NOT NULL,
    plan_id         TEXT REFERENCES plans(id),
    sortie_id       TEXT REFERENCES sorties(id),
    drone_id        TEXT,
    payload         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_log_ts_idx     ON audit_log(ts);
CREATE INDEX audit_log_plan_idx   ON audit_log(plan_id);
CREATE INDEX audit_log_sortie_idx ON audit_log(sortie_id);
CREATE INDEX audit_log_actor_idx  ON audit_log(actor);
CREATE INDEX audit_log_event_idx  ON audit_log(event);
