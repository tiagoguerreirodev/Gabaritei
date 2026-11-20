import { render, screen } from "@solidjs/testing-library";
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import QuestionImage, { imageUrl } from "./QuestionImage";

const name = "0095e076de67503b10dc9bb88ddaa5bc.png";

describe("QuestionImage", () => {
  it("points at the image scheme on Linux and macOS", () => {
    mockConvertFileSrc("linux");
    expect(imageUrl(name)).toBe(`image://localhost/${name}`);
  });

  it("points at the image scheme's localhost host on Windows", () => {
    mockConvertFileSrc("windows");
    expect(imageUrl(name)).toBe(`http://image.localhost/${name}`);
  });

  it("renders the image with its alt text", () => {
    mockConvertFileSrc("linux");
    render(() => <QuestionImage name={name} alt="Cabeçalho IPv4 com as posições dos campos." />);

    const image = screen.getByRole("img", { name: "Cabeçalho IPv4 com as posições dos campos." });
    expect(image).toHaveAttribute("src", `image://localhost/${name}`);
    expect(image).toHaveAttribute("loading", "lazy");
  });
});
