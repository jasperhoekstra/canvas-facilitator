import { expect, test } from "vitest";
import { clock, usd } from "./format";

test("clock rounds up and never goes negative", () => {
  expect(clock(900_000)).toBe("15:00");
  expect(clock(762_001)).toBe("12:43");
  expect(clock(-5)).toBe("0:00");
});

test("usd uses Dutch decimals", () => {
  expect(usd("0.28")).toBe("$0,28");
  expect(usd("1.6318", 3)).toBe("$1,632");
});
