import { describe, expect, it } from "vitest";
import { isOptimizedImageUrl } from "./market-thumb";

describe("isOptimizedImageUrl", () => {
  it("accepts only query-free HTTPS artwork from the configured bucket", () => {
    expect(
      isOptimizedImageUrl(
        "https://polymarket-upload.s3.us-east-2.amazonaws.com/events/thumb.png",
      ),
    ).toBe(true);
    expect(
      isOptimizedImageUrl(
        "http://polymarket-upload.s3.us-east-2.amazonaws.com/events/thumb.png",
      ),
    ).toBe(false);
    expect(
      isOptimizedImageUrl(
        "https://polymarket-upload.s3.us-east-2.amazonaws.com/events/thumb.png?v=2",
      ),
    ).toBe(false);
  });

  it("does not treat lookalike, unrelated, or malformed URLs as optimizable", () => {
    expect(
      isOptimizedImageUrl(
        "https://polymarket-upload.s3.us-east-2.amazonaws.com.evil.test/thumb.png",
      ),
    ).toBe(false);
    expect(isOptimizedImageUrl("https://example.com/thumb.png")).toBe(false);
    expect(isOptimizedImageUrl("not a URL")).toBe(false);
  });
});
