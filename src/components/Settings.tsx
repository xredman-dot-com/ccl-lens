import { useEffect, useState } from "react";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

export function Settings() {
  const [on, setOn] = useState(false);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    isEnabled()
      .then((v) => setOn(v))
      .catch(() => {})
      .finally(() => setReady(true));
  }, []);

  const toggle = async () => {
    try {
      if (on) {
        await disable();
        setOn(false);
      } else {
        await enable();
        setOn(true);
      }
    } catch {
      /* ignore */
    }
  };

  return (
    <section className="settings">
      <label className="switch-row">
        <input type="checkbox" checked={on} disabled={!ready} onChange={toggle} />
        <span>开机自动启动</span>
      </label>
    </section>
  );
}
