// User-Agent for all outbound HTTP requests (http(s):// and pmtiles://http(s)://).
// By default we don't override the User-Agent at all (the runtime's default is
// used). Set one explicitly via --user-agent, CHIITILER_USER_AGENT, or
// setUserAgent() — e.g. some tile providers (OpenStreetMap) require one.
let overrideUserAgent: string | undefined;

/**
 * Override the User-Agent sent with outbound HTTP requests.
 * Takes precedence over the CHIITILER_USER_AGENT environment variable.
 */
function setUserAgent(userAgent: string | undefined) {
    overrideUserAgent = userAgent;
}

/**
 * Returns the configured User-Agent, or undefined when none is set (in which
 * case no User-Agent header should be added).
 */
function getUserAgent(): string | undefined {
    return overrideUserAgent ?? process.env.CHIITILER_USER_AGENT;
}

export { getUserAgent, setUserAgent };
