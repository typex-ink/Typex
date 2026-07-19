import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { describe, expect, it } from "vitest";
import { siteCopy } from "../content";
import SiteHeader from "./SiteHeader.vue";

describe("site header", () => {
  it("closes the mobile menu with Escape and returns focus to the menu button", async () => {
    const wrapper = mount(SiteHeader, {
      attachTo: document.body,
      props: {
        copy: siteCopy.en.nav,
        locale: "en",
        theme: "light",
      },
    });

    const button = wrapper.get("button.menu-control");
    await button.trigger("click");
    expect(button.attributes("aria-expanded")).toBe("true");
    expect(wrapper.find("#mobile-navigation").exists()).toBe(true);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await nextTick();
    await nextTick();

    expect(wrapper.find("#mobile-navigation").exists()).toBe(false);
    expect(document.activeElement).toBe(button.element);
    wrapper.unmount();
  });
});
