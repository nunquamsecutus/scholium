import { describe, it, expect } from "vitest";
import { reorder } from "./reorder";

describe("reorder", () => {
  it("moves an item forward", () => {
    expect(reorder(["a", "b", "c", "d"], 0, 2)).toEqual(["b", "c", "a", "d"]);
  });

  it("moves an item backward", () => {
    expect(reorder(["a", "b", "c", "d"], 3, 1)).toEqual(["a", "d", "b", "c"]);
  });

  it("returns the list unchanged when from equals to", () => {
    const arr = ["a", "b", "c"];
    expect(reorder(arr, 1, 1)).toBe(arr);
  });

  it("returns the list unchanged for out-of-range indices", () => {
    const arr = ["a", "b", "c"];
    expect(reorder(arr, -1, 0)).toBe(arr);
    expect(reorder(arr, 0, 5)).toBe(arr);
  });

  it("does not mutate the input", () => {
    const arr = ["a", "b", "c"];
    const out = reorder(arr, 0, 2);
    expect(arr).toEqual(["a", "b", "c"]);
    expect(out).toEqual(["b", "c", "a"]);
  });
});
