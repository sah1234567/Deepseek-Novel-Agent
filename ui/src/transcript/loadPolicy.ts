/**
 * Transcript turn memory & scroll-anchor policy.
 *
 * Budget unit is **turn** (one user message + assistant/tool chain), not individual bubbles.
 * Older turns are never deleted from DB — only unloaded from the FSM; idle slots reload on scroll.
 *
 * Three-tier memory model:
 * - TAIL_LOADED_TURNS: bootstrap + bottom-anchored tail retention window.
 * - VIEW_LOADED_TURNS: browse-mode sliding window width around visible focal.
 * - MAX_LOADED_TURNS: hard cap while browsing; overflow eviction beyond this.
 *
 * - TAIL_CONTENT_UNDERFLOW_PX: bottom-anchored underfill threshold for upward prefetch.
 * - BOTTOM_ANCHOR_THRESHOLD_PX: shared near-bottom zone for scroll follow + tail compaction.
 * - TAIL_COMPACT_DEBOUNCE_MS: debounce before reconcileMemoryWindow at bottom anchor.
 */

/** Bootstrap target and tail window after returning to bottom anchor. */
export const TAIL_LOADED_TURNS = 6;

/** Browse-mode sliding window width around visible focal index. */
export const VIEW_LOADED_TURNS = 6;

export const TURN_LOAD_BATCH = 3;

/** Max loaded turns in FSM while browsing; overflow eviction beyond this. */
export const MAX_LOADED_TURNS = 18;

export const INTERSECTION_ROOT_MARGIN = "240px 0px";

/**
 * Distance from scroll bottom (px) treated as "near bottom":
 * scroll follow (new bubbles snap to bottom) + bottom-anchor compaction trigger.
 */
export const BOTTOM_ANCHOR_THRESHOLD_PX = 128;

/** Debounce before reconcileMemoryWindow compacts to TAIL_LOADED_TURNS at bottom anchor. */
export const TAIL_COMPACT_DEBOUNCE_MS = 400;

/**
 * When bottom-anchored, scrollHeight below viewport + this margin triggers upward prefetch.
 */
export const TAIL_CONTENT_UNDERFLOW_PX = 48;

/** Placeholder height while a turn slot is idle/evicted (reduces scroll jump). */
export const TURN_PLACEHOLDER_MIN_HEIGHT = "4rem";
