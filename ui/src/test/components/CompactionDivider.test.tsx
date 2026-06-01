import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { CompactionDivider } from "../../components/chat/CompactionDivider";

describe("CompactionDivider", () => {
  it("shows epoch label", () => {
    render(<CompactionDivider epoch={2} />);
    expect(screen.getByText(/第 2 次/)).toBeInTheDocument();
  });
});
