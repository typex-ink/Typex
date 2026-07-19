import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { siteCopy } from "../content";
import ProductDemo from "./ProductDemo.vue";

const canvasContext = {
  clearRect: vi.fn(),
  beginPath: vi.fn(),
  roundRect: vi.fn(),
  fill: vi.fn(),
  fillStyle: "",
  globalAlpha: 1,
};

describe("product demo", () => {
  beforeEach(() => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      canvasContext as unknown as CanvasRenderingContext2D,
    );
  });

  afterEach(() => vi.restoreAllMocks());

  it("renders the static completed state when reduced motion is requested", async () => {
    const animationFrame = vi.spyOn(window, "requestAnimationFrame");
    const wrapper = mount(ProductDemo, { props: { copy: siteCopy.en.demo } });
    await nextTick();

    expect(wrapper.get(".product-demo").attributes("data-phase")).toBe("complete");
    expect(wrapper.text()).toContain(siteCopy.en.demo.typed);
    expect(wrapper.text()).toContain(siteCopy.en.demo.result);
    expect(wrapper.find("button.demo-control").exists()).toBe(false);
    expect(animationFrame).not.toHaveBeenCalled();

    wrapper.unmount();
  });
});
