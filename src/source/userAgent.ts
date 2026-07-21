// User-Agent for all outbound HTTP requests (http(s):// and pmtiles://http(s)://).
// Some tile providers (e.g. OpenStreetMap) reject requests without an
// identifying User-Agent, so we always send one.
const DEFAULT_USER_AGENT = 'chiitiler (+https://github.com/Kanahiro/chiitiler)';

let overrideUserAgent: string | undefined;

/**
 * Override the User-Agent sent with outbound HTTP requests.
 * Takes precedence over the CHIITILER_USER_AGENT environment variable.
 */
function setUserAgent(userAgent: string | undefined) {
    overrideUserAgent = userAgent;
}

function getUserAgent(): string {
    return (
        overrideUserAgent ??
        process.env.CHIITILER_USER_AGENT ??
        DEFAULT_USER_AGENT
    );
}

export { getUserAgent, setUserAgent };
