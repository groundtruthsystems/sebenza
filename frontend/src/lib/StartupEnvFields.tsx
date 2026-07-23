import Toggle from "./Toggle";

export default function StartupEnvFields({
  startupEnvs = {},
  envValues = {},
  onEnvValuesChange,
}: {
  startupEnvs: Record<string, string | boolean>;
  envValues: Record<string, string | boolean>;
  onEnvValuesChange?: (next: Record<string, string | boolean>) => void;
}) {
  const envKeys = Object.keys(startupEnvs);

  if (envKeys.length === 0) return null;

  return (
    <div className="mb-4">
      <p className="text-xs text-muted mb-2">
        Startup Environment <span className="opacity-60">(optional)</span>
      </p>
      <div className="pl-3 border-l border-edge">
        {envKeys.map((key) => {
          if (typeof startupEnvs[key] === "boolean") {
            return (
              <div key={key} className="mb-3">
                <label className="flex items-center gap-2 text-xs text-muted cursor-pointer">
                  <Toggle
                    aria-label={key}
                    checked={envValues[key] === true}
                    onToggle={(v) => onEnvValuesChange?.({ ...envValues, [key]: v })}
                  />
                  {key}
                </label>
              </div>
            );
          }
          const value = envValues[key];
          return (
            <div key={key} className="mb-3">
              <label className="block text-xs text-muted mb-1.5" htmlFor={`wt-env-${key}`}>
                {key}
              </label>
              <input
                id={`wt-env-${key}`}
                type="text"
                className="w-full px-2.5 py-1.5 rounded-md border border-edge bg-surface text-primary text-[13px] placeholder:text-muted/50 outline-none focus:border-accent"
                value={typeof value === "string" ? value : ""}
                onChange={(e) => onEnvValuesChange?.({ ...envValues, [key]: e.target.value })}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
