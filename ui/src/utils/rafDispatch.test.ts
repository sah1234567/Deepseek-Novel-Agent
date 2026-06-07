import { beforeEach, describe, expect, it, vi } from "vitest";
import { createRafBatcher } from "./rafDispatch";

describe("createRafBatcher", () => {
  beforeEach(() => {
    vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
  });

  it("flushes queued items on flushNow", () => {
    const flush = vi.fn();
    const batcher = createRafBatcher<number>(flush);
    batcher.push(1);
    batcher.push(2);
    batcher.flushNow();
    expect(flush).toHaveBeenCalledWith([1, 2]);
  });
});
