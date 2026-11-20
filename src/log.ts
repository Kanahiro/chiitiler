import { inspect } from 'node:util';

type Severity = 'INFO' | 'WARNING' | 'ERROR';

/**
 * One JSON line per entry, which Cloud Logging, CloudWatch and the like parse (`severity`, `message`).
 * Errors among the fields are written with their stack and causes.
 */
function log(
    severity: Severity,
    message: string,
    fields: Record<string, unknown> = {},
) {
    const line = JSON.stringify({ severity, message, ...fields }, (_, v) =>
        v instanceof Error ? inspect(v) : v,
    );
    if (severity === 'INFO') console.log(line);
    else console.error(line);
}

export { log };
