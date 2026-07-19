import { mount } from "@vue/test-utils";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { LOCALE_STORAGE_KEY, THEME_STORAGE_KEY } from "./preferences";
import { SITE_LINKS } from "./links";

const mediaQuery = {
  matches: false,
  media: "",
  onchange: null,
  addListener: vi.fn(),
  removeListener: vi.fn(),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
  dispatchEvent: vi.fn(() => true),
} as unknown as MediaQueryList;

let App: (typeof import("./App.vue"))["default"];

beforeAll(async () => {
  window.localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => mediaQuery),
  });
  App = (await import("./App.vue")).default;
});

describe("website app", () => {
  it("renders platform downloads and persists language and theme controls", async () => {
    const wrapper = mount(App, {
      global: {
        stubs: {
          ProductDemo: { template: '<div data-test="product-demo" />' },
          FeatureSection: { template: '<div data-test="feature-section" />' },
        },
      },
    });

    const releaseLinks = wrapper
      .findAll("a")
      .filter((link) => link.attributes("href") === SITE_LINKS.releases);
    expect(releaseLinks).toHaveLength(2);
    expect(wrapper.text()).toContain("macOS 12 or later");
    expect(wrapper.text()).toContain("Windows 10 22H2+ / Windows 11 x64");

    await wrapper.get('button[title="Use dark theme"]').trigger("click");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");

    await wrapper.get("button.locale-control").trigger("click");
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(document.title).toBe("Typex - 说，即所得。");

    wrapper.unmount();
  });
});
