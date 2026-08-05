import { describe, expect, it } from "vitest";
import { formatProfileTestError } from "./provider-error";

describe("formatProfileTestError", () => {
  it("pretty-prints JSON details without dropping unknown fields", () => {
    const raw = JSON.stringify({
      error: {
        message: "Upstream request failed",
        details: { error: { message: "model not found" } },
        request_id: "req-123",
      },
    });

    const display = formatProfileTestError({
      code: "invalid_request",
      message: "Upstream request failed",
      details: raw,
    });

    expect(display.message).toBe("Upstream request failed");
    expect(display.details).toBe(JSON.stringify(JSON.parse(raw), null, 2));
    expect(display.details).toContain("req-123");
    expect(display.details).toContain("model not found");
  });

  it("keeps non-JSON details unchanged and avoids a duplicate summary", () => {
    const raw = "route not found\nrequest: req-456\n";
    const display = formatProfileTestError({
      code: "invalid_request",
      message: raw.trim(),
      details: raw,
    });

    expect(display.message).toBe("");
    expect(display.details).toBe(raw);
  });
});
