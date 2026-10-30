import { View, div } from "gpui-kit";
import { v_flex } from "gpui-base";
import { appHome, readTextFile, exec, theme, props, nextCommand, setShared } from "herdr";

export default class Digest extends View {
  init(_props, cx) {
    this.data = {items: [], status: "Waiting for the first scheduled digest"};
    this.finished = false;
    this.error = "";
    this.demo = !!props()?.demo;
    if (this.demo) this.command("sample", cx);
    else this.reload(cx);
    cx.timer.every(60000, cx => this.reload(cx));
    cx.spawn(async cx => {
      for (;;) { this.command(await nextCommand(), cx); cx.notify(); }
    });
  }
  debug(last = "load") {
    setShared("digest.debug", {last, demo: this.demo, finished: this.finished, error: this.error, ...this.data});
  }
  reload(cx) {
    if (this.demo) return;
    cx.spawn(async cx => {
      try {
        const raw = await readTextFile(`${appHome()}/data/digest/latest.json`);
        if (raw) {
          const next = JSON.parse(raw);
          if (!Array.isArray(next.items)) throw new Error("Invalid digest file");
          if (next.id !== this.data.id) this.finished = localStorage.getItem("finished") === next.id;
          this.data = next;
        }
        const status = await readTextFile(`${appHome()}/data/digest/status.json`);
        this.error = status ? JSON.parse(status).error || "" : "";
      } catch (e) { this.error = String(e); }
      this.debug(); cx.notify();
    });
  }
  command(command, cx) {
    if (command === "finish") {
      this.finished = true;
      if (!this.demo && this.data.id) localStorage.setItem("finished", this.data.id);
    } else if (command === "sample" && this.demo) {
      this.finished = false;
      this.error = "";
      this.data = {id:"sample", status:"Preview · sample content", next:"Today at 18:00", sources:["X · followed accounts · sample"], items:[
        {title:"A release with a useful new capability", summary:"A followed developer announced a release and explained the practical change. Repeated reactions have been left out of this selection.", why:"A concrete update with enough detail to assess.", source:"X · sample", url:"https://x.com"},
        {title:"A technical explanation worth keeping", summary:"A followed account shared a focused explanation of a design tradeoff. This is sample text used to check the reading experience.", why:"Substantive context, without the surrounding conversation.", source:"X · sample", url:"https://x.com"}
      ]};
    } else if (command === "empty" && this.demo) {
      this.finished = false; this.error = ""; this.data = {status:"Preview · no relevant updates", sources:["X · followed accounts · sample"], items:[], next:"Today at 18:00"};
    } else if (command === "failure" && this.demo) {
      this.finished = false;
      this.error = "Sample connection failure. The previous digest is preserved.";
      this.data = {...this.data, status:"Collection needs attention", sources:["X · connection failed · sample"]};
    } else if (command.startsWith("open:")) {
      const item = this.data.items[Number(command.slice(5))];
      if (item && /^https:\/\//.test(item.url)) cx.spawn(async cx => {
        try { const r = await exec("xdg-open", [item.url]); if (r.code) this.error = "Could not open source"; }
        catch (e) { this.error = String(e); }
        this.debug(command); cx.notify();
      });
    }
    this.debug(command); cx.notify();
  }
  button(label, command, cx) {
    const t = theme();
    return div().id(command).px(14).py(8).rounded(6).bg(t.raised).text_color(t.foreground)
      .hover(s => s.bg(t.hover)).child(label).on_click((_e, cx) => this.command(command, cx));
  }
  render(cx) {
    const t = theme();
    return v_flex().id("digest-root").size_full().overflow_y_scrollbar().bg(t.canvas).p(24).gap(18).text_size(14).text_color(t.foreground).children([
      div().text_size(12).text_color(t.accent).child("A LITTLE LESS NOISE"),
      div().text_size(28).font_semibold().child("Your digest"),
      div().text_color(t.muted).child(this.data.status || "Your scheduled selection"),
      div().text_color(t.dim).child(`Next digest: ${this.data.next || "09:00 / 18:00 · local time"}`),
      ...(this.data.sources || ["X · followed accounts · awaiting setup"]).map(s => div().text_size(12).text_color(t.muted).child(s)),
      ...(this.error ? [div().text_color(t.blocked).child(this.error)] : []),
      ...(this.finished ? [div().text_size(22).child("You’re caught up."), div().text_color(t.muted).child("Close this panel. The next digest will arrive on schedule.")] : this.data.items.map((item, i) =>
        v_flex().flex_shrink_0().p(18).gap(10).rounded(10).bg(t.surface).children([
          div().text_size(12).text_color(t.accent).child(`${i + 1} / ${this.data.items.length} · ${item.source}`),
          div().text_size(19).font_semibold().child(item.title),
          div().child(item.summary),
          div().text_color(t.muted).child(`Why it matters: ${item.why}`),
          ...(item.url ? [this.button("Open original", `open:${i}`, cx)] : [])
        ]))),
      ...(!this.finished && this.data.items.length ? [this.button("Done for now", "finish", cx)] : []),
      ...(!this.finished && !this.error && !this.data.items.length ? [div().text_size(20).child("Nothing to read right now."), div().text_color(t.muted).child("No feed to refresh. New selections appear at the next scheduled digest.")] : [])
    ]);
  }
}
