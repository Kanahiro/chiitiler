import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// Keep npm and container publication on the same release channel. A prerelease
// version must never become latest, even if the GitHub checkbox is omitted.
export function releaseConfig(version, eventName, event) {
    if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
        throw new Error(`Unsupported release version: ${version}`);
    }
    const prerelease = version.includes('-');
    let imageTag = `v${version}`;
    let npmTag;
    if (eventName === 'release') {
        imageTag = event.release?.tag_name;
        if (imageTag !== version && imageTag !== `v${version}`) {
            throw new Error('Release tag must match package.json version');
        }
        npmTag = event.release.prerelease || prerelease ? 'next' : 'latest';
    } else if (eventName === 'workflow_dispatch') {
        npmTag = event.inputs?.channel;
        if (!['next', 'latest'].includes(npmTag)) {
            throw new Error('Select the next or latest release channel');
        }
        if (prerelease && npmTag === 'latest') {
            throw new Error('A prerelease version cannot be published as latest');
        }
    } else {
        throw new Error(`Unsupported release event: ${eventName}`);
    }
    return { image_tag: imageTag, npm_tag: npmTag, publish_latest: String(npmTag === 'latest') };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
    const { version } = JSON.parse(readFileSync('package.json', 'utf8'));
    const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
    const config = releaseConfig(version, process.env.GITHUB_EVENT_NAME, event);
    appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(config).map(([k, v]) => `${k}=${v}\n`).join(''));
    console.log(config);
}
