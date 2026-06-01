import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { CompactionBanner } from "../../components/chat/CompactionBanner";

describe("CompactionBanner", () => {
  it("renders nothing when not visible", () => {
    const { container } = render(
      <CompactionBanner
        state={{ visible: false, action: "started", variant: "info" }}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows generating-summary with spinner", () => {
    render(
      <CompactionBanner
        state={{ visible: true, action: "generating-summary", variant: "info" }}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("摘要");
  });

  it("shows failed reason", () => {
    render(
      <CompactionBanner
        state={{
          visible: true,
          action: "failed",
          variant: "warn",
          reason: "timeout",
        }}
      />,
    );
    expect(screen.getByText(/timeout/)).toBeInTheDocument();
  });
});
