import { View, div, svg } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { appHome, home, readTextFile, watchFolder, exec, theme, props, nextCommand, setShared, run } from "herdr";

// The zones, in the order the panel shows them; each is collected into data/digest/<key>.json.
const ZONES = [
  {key: "slack", name: "Slack", color: "ansi4"},
  {key: "email", name: "Email", color: "ansi3"},
  {key: "social", name: "Social", color: "ansi5"},
];
const ICONS = "mods/digest";

/** What demo mode shows: every zone with items, every zone empty, or two zones failing. */
function sample(kind) {
  const at = "2026-11-05T08:00:00+00:00";
  const item = (id, title, summary, why, source, url) => ({id, title, summary, why, source, url, at});
  const zones = {
    slack: {refreshed: "Thu 17:30", next: "Thu 18:00", coverage: "38 inspected", error: "", items: [
      item("slack:1", "The release moves to Friday", "The platform team pushed the release by a day to finish the migration checks. QA sign-off is now due Thursday evening.", "Your review slot moves with it.", "Slack · Ana Lopez", "https://example.slack.com/archives/C1/p1"),
      item("slack:2", "A direct question about the cache budget", "Sam asks whether the cache can drop to 2 GB per node before the capacity review, and needs an answer today.", "A question addressed to you, with a deadline.", "Slack · Sam Okafor", "https://example.slack.com/archives/C1/p2"),
    ]},
    email: {refreshed: "Thu 08:00", next: "Fri 08:00", coverage: "12 inspected", error: "", items: [
      item("email:1", "A decision waiting in your inbox", "Claire asks you to choose between two dates for the offsite, before tomorrow noon.", "A concrete next action with a deadline.", "Email · Claire Martin", "https://mail.google.com/mail/u/0/#inbox/sample"),
    ]},
    social: {refreshed: "Thu 08:00", next: "Fri 08:00", coverage: "42 new posts", error: "", items: [
      item("x:1", "A release with a useful new capability", "A followed developer announced a release and explained the practical change it brings, with a short migration note.", "A concrete update with enough detail to assess.", "X · sample_dev", "https://x.com"),
      item("x:2", "A careful thread on a design tradeoff", "A followed engineer weighs a queue against a log for event delivery, and says where each one breaks.", "Substantive context, without the surrounding argument.", "X · sample_eng", "https://x.com"),
    ]},
  };
  if (kind === "empty") {
    for (const zone of Object.values(zones)) { zone.items = []; zone.coverage = "Nothing new"; }
  } else if (kind === "failure") {
    zones.slack.error = "The Enough Internet helper stopped or needs input; inspect it in Herdr";
    zones.slack.checked = "Thu 18:00";
    zones.social = {next: "Fri 08:00", checked: "Thu 08:00", error: "X collection failed; check token access, credits and rate limits", items: []};
  }
  return zones;
}

export default class Digest extends View {
  init(_props, cx) {
    this.zones = {};
    this.progress = {};
    this.error = "";
    this.refreshing = {};
    // Why a zone's last Refresh did not run, apart from the failures its file records.
    this.refreshErrors = {};
    this.demo = !!props()?.demo;
    this.marks = {};
    if (this.demo) this.command("sample", cx);
    else {
      for (const {key} of ZONES) this.marks[key] = this.stored(key);
      this.reload(cx);
    }
    // The collector's writes show at once; the timer is the fallback the folder watch needs.
    cx.timer.every(15000, cx => this.reload(cx));
    if (!this.demo) cx.spawn(async cx => {
      try { for (;;) { await watchFolder(`${appHome()}/data/digest`); this.reload(cx); } } catch {}
    });
    cx.spawn(async cx => {
      for (;;) { this.command(await nextCommand(), cx); cx.notify(); }
    });
  }

  /** The zone's read mark; the panel before the zones kept one for its whole digest, which the
   *  items it moved into the zones carry as their `at`. */
  stored(key) {
    try { return localStorage.getItem(`read:${key}`) || localStorage.getItem("finished") || ""; }
    catch { return ""; }
  }

  /** The zone's items not marked read: those collected after its "Done for now". */
  unread(key) {
    const mark = this.marks[key] || "";
    return (this.zones[key]?.items || []).filter(item => !mark || (item.at || "") > mark);
  }

  debug(last = "load") {
    const zones = {};
    for (const {key} of ZONES) {
      const zone = this.zones[key];
      zones[key] = zone ? {
        refreshed: zone.refreshed || "", next: zone.next || "", coverage: zone.coverage || "",
        error: zone.error || "", refresh_error: this.refreshErrors[key] || "", items: this.unread(key).map(({title, source}) => ({title, source})),
        refreshing: !!this.refreshing[key],
      } : null;
    }
    const unread = ZONES.reduce((n, {key}) => n + this.unread(key).length, 0);
    setShared("digest.debug", {last, demo: this.demo, error: this.error, progress: this.progress, unread, finished: unread === 0, zones});
  }

  reload(cx) {
    if (this.demo) return;
    cx.spawn(async cx => {
      const data = `${appHome()}/data/digest`;
      try {
        for (const {key} of ZONES) {
          const raw = await readTextFile(`${data}/${key}.json`);
          if (!raw) continue;
          const zone = JSON.parse(raw);
          if (zone.items !== undefined && !Array.isArray(zone.items)) throw new Error(`Invalid ${key} file`);
          this.zones[key] = zone;
        }
        const progress = await readTextFile(`${data}/progress.json`);
        this.progress = progress ? JSON.parse(progress) : {};
        this.error = "";
      } catch (e) { this.error = String(e); }
      this.debug(); cx.notify();
    });
  }

  refresh(key, cx) {
    if (this.refreshing[key] || !ZONES.some(z => z.key === key)) return;
    if (this.demo) return;
    this.refreshing[key] = true;
    this.refreshErrors[key] = "";
    cx.spawn(async cx => {
      try {
        const r = await exec(`${home()}/.local/bin/herdr-digest`, ["collect", "--zone", key]);
        if (r.stdout.includes("Another collection is running")) {
          this.refreshErrors[key] = "Already collecting; try again shortly.";
        } else if (r.code && !r.stdout.trim()) {
          this.refreshErrors[key] = "Refresh failed; run herdr-digest collect to see why.";
        }
      } catch (e) { this.refreshErrors[key] = `Could not run herdr-digest: ${e}`; }
      this.refreshing[key] = false;
      this.reload(cx);
      this.debug(`refreshed:${key}`); cx.notify();
    });
  }

  finish() {
    for (const {key} of ZONES) {
      const items = this.zones[key]?.items || [];
      if (!items.length) continue;
      const newest = items.reduce((max, item) => (item.at || "") > max ? item.at : max, "");
      this.marks[key] = newest;
      if (!this.demo) try { localStorage.setItem(`read:${key}`, newest); } catch {}
    }
  }

  command(command, cx) {
    const [name, ...args] = command.split(":");
    if (name === "peek" && this.progress.helper) {
      run("terminal.peek", {terminal: this.progress.helper});
    } else if (name === "finish") {
      this.finish();
    } else if (name === "refresh") {
      this.refresh(args[0], cx);
    } else if (["sample", "empty", "failure"].includes(name) && this.demo) {
      this.zones = sample(name);
      this.marks = {};
      this.error = "";
    } else if (name === "open") {
      const item = this.unread(args[0])[Number(args[1])];
      if (item && /^https:\/\//.test(item.url)) cx.spawn(async cx => {
        try { const r = await exec("xdg-open", [item.url]); if (r.code) this.error = "Could not open source"; }
        catch (e) { this.error = String(e); }
        this.debug(command); cx.notify();
      });
    }
    this.debug(command); cx.notify();
  }

  icon(name, size, color) {
    return svg(`${ICONS}/${name}.svg`).size(size).flex_none().text_color(color);
  }

  /** A zone's header: its name, when it last refreshed and refreshes next, and Refresh. */
  header(zone, state, cx) {
    const t = theme();
    const progress = this.progress.zone === zone.key && this.progress.phase !== "Ready";
    const busy = this.refreshing[zone.key] || progress;
    const when = !state ? "Not collected yet"
      : [state.refreshed ? `Updated ${state.refreshed}` : "Not collected yet", state.next ? `next ${state.next}` : ""].filter(Boolean).join(" · ");
    return h_flex().items_start().gap(10).px(18).pt(14).pb(12).children([
      div().size(8).mt(7).rounded_full().flex_none().bg(t[zone.color] || t.accent),
      v_flex().flex_1().min_w(0).gap(2).children([
        div().text_size(15).font_semibold().text_color(t.foreground).child(zone.name),
        busy
          ? h_flex().items_center().gap(6).children([
              this.icon("spinner", 12, t.accent).spin(900),
              div().text_size(12).text_color(t.accent).truncate().child(progress ? `${this.progress.phase}…` : "Refreshing…"),
            ])
          : div().text_size(12).text_color(t.dim).child(when),
      ]),
      div().id(`refresh-${zone.key}`).size(28).flex_none().rounded(6).flex().items_center().justify_center()
        .cursor_pointer().hover(s => s.bg(t.hover)).tooltip(`Refresh ${zone.name}`)
        .child(this.icon("refresh", 14, busy ? t.dim : t.muted))
        .on_click((_e, cx) => this.command(`refresh:${zone.key}`, cx)),
    ]);
  }

  failure(state) {
    const t = theme();
    return h_flex().items_start().gap(10).mx(18).mb(12).px(12).py(10).rounded(8).bg(t.blocked + "14").children([
      div().pt(2).child(this.icon("alert", 14, t.blocked)),
      v_flex().flex_1().min_w(0).gap(2).children([
        div().text_size(12.5).font_semibold().text_color(t.blocked).child(state.checked ? `Couldn’t refresh at ${state.checked}` : "Couldn’t refresh"),
        div().text_size(12.5).text_color(t.muted).child(state.error),
        div().text_size(12).text_color(t.dim).child(state.items?.length ? "Showing the last selection." : "Refresh to try again."),
      ]),
    ]);
  }

  item(zone, item, i, last) {
    const t = theme();
    return v_flex().id(`open-${zone.key}-${i}`).gap(6).mx(8).px(10).py(12).rounded(8)
      .cursor_pointer().hover(s => s.bg(t.hover))
      .when(!last, el => el.border_b(1).border_color(t.border))
      .on_click((_e, cx) => this.command(`open:${zone.key}:${i}`, cx))
      .children([
        h_flex().items_start().gap(10).children([
          div().flex_1().min_w(0).text_size(15).font_semibold().text_color(t.foreground).child(item.title),
          div().pt(3).child(this.icon("open", 13, t.dim)),
        ]),
        div().text_size(13.5).text_color(t.muted).child(item.summary),
        div().border_l(2).border_color(t.border).pl(8).text_size(12.5).text_color(t.dim).child(item.why),
        div().pt(2).text_size(11.5).font_medium().text_color(t.dim).child(item.source),
      ]);
  }

  /** A zone's card: its header, its failure, then its unread items, or why it has none. */
  zone(zone, cx) {
    const t = theme();
    const state = this.zones[zone.key];
    const unread = this.unread(zone.key);
    const body = [];
    if (state?.error) body.push(this.failure(state));
    if (this.refreshErrors[zone.key]) {
      body.push(div().px(18).pb(10).text_size(12.5).text_color(t.blocked).child(this.refreshErrors[zone.key]));
    }
    if (unread.length) {
      body.push(...unread.map((item, i) => this.item(zone, item, i, i === unread.length - 1)));
    } else if (state?.items?.length) {
      body.push(h_flex().items_center().gap(8).px(18).pb(16).children([
        this.icon("check", 14, t.done), div().text_size(13).text_color(t.muted).child("Caught up"),
      ]));
    } else if (state && !state.error) {
      body.push(div().px(18).pb(16).text_size(13).text_color(t.muted).child("Nothing worth your time."));
    } else if (!state) {
      body.push(div().px(18).pb(16).text_size(13).text_color(t.muted).child("Waiting for its first collection."));
    }
    const coverage = state?.coverage && unread.length
      ? [div().px(18).pt(4).pb(14).text_size(11.5).text_color(t.dim).child(state.coverage)] : [];
    return v_flex().flex_shrink_0().rounded(12).border(1).border_color(t.border).bg(t.background).pb(unread.length ? 4 : 0)
      .children([this.header(zone, state, cx), ...body, ...coverage]);
  }

  render(cx) {
    const t = theme();
    const unread = ZONES.reduce((n, {key}) => n + this.unread(key).length, 0);
    const waiting = ZONES.every(({key}) => !this.zones[key]);
    const empty = ZONES.every(({key}) => !this.zones[key]?.items?.length);
    const headline = waiting ? "Nothing yet"
      : unread ? `${unread} ${unread === 1 ? "thing" : "things"} worth your time`
      : empty ? "Nothing worth your time." : "You’re caught up.";
    const subline = waiting ? "Your first selections arrive with the next collection."
      : unread ? "A few selections from Slack, email and social media. Then you’re done."
      : "Enough internet for today. New selections arrive on schedule.";
    return div().id("digest-root").size_full().overflow_y_scrollbar().bg(t.canvas).child(
      v_flex().w_full().max_w(680).mx_auto().px(20).pt(28).pb(32).gap(14).children([
        v_flex().gap(6).px(2).pb(6).children([
          div().text_size(11).font_semibold().text_color(t.dim).child("ENOUGH INTERNET"),
          div().text_size(24).font_semibold().text_color(t.foreground).child(headline),
          div().text_size(13).text_color(t.muted).child(subline),
        ]),
        ...(this.error ? [div().px(2).text_size(12.5).text_color(t.blocked).child(this.error)] : []),
        ...ZONES.map(zone => this.zone(zone, cx)),
        ...(unread ? [
          div().id("finish").mt(6).h(42).rounded(8).flex().items_center().justify_center().cursor_pointer()
            .bg(t.solid).text_color(t.solid_foreground).text_size(14).font_semibold()
            .hover(s => s.opacity(0.9)).child("Done for now")
            .on_click((_e, cx) => this.command("finish", cx)),
        ] : []),
        ...(this.progress.helper ? [
          div().id("peek").self_center().px(10).py(4).rounded(6).cursor_pointer().text_size(12).text_color(t.dim)
            .hover(s => s.bg(t.hover).text_color(t.muted)).child("Inspect the Sonnet helper")
            .on_click((_e, cx) => this.command("peek", cx)),
        ] : []),
      ]),
    );
  }
}
