import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, describe, expect, it, vi } from "vitest";
import { siteCopy } from "../content";
import FeatureVisual from "./FeatureVisual.vue";

let intersectionCallback: IntersectionObserverCallback;

class IntersectionObserverMock {
  readonly disconnect = vi.fn();
  readonly observe = vi.fn();

  constructor(callback: IntersectionObserverCallback) {
    intersectionCallback = callback;
  }
}

function setupMotionPreference(reduced: boolean): void {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => ({
      matches: reduced,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  });
  vi.stubGlobal(
    "IntersectionObserver",
    IntersectionObserverMock as unknown as typeof IntersectionObserver,
  );
}

function reportIntersection(isIntersecting: boolean): void {
  intersectionCallback(
    [{ isIntersecting } as IntersectionObserverEntry],
    {} as IntersectionObserver,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("feature visual animation", () => {
  it("activates in view and resets when it leaves the viewport", async () => {
    setupMotionPreference(false);
    const wrapper = mount(FeatureVisual, {
      props: { feature: siteCopy.en.features[1] },
    });

    expect(wrapper.classes()).not.toContain("feature-visual--active");
    reportIntersection(true);
    await nextTick();
    expect(wrapper.classes()).toContain("feature-visual--active");

    reportIntersection(false);
    await nextTick();
    expect(wrapper.classes()).not.toContain("feature-visual--active");
    wrapper.unmount();
  });

  it("stays static when reduced motion is requested", async () => {
    setupMotionPreference(true);
    const wrapper = mount(FeatureVisual, {
      props: { feature: siteCopy.en.features[1] },
    });

    reportIntersection(true);
    await nextTick();
    expect(wrapper.classes()).not.toContain("feature-visual--active");
    expect(wrapper.text()).toContain(siteCopy.en.features[1].visual.valueB);
    wrapper.unmount();
  });
});
