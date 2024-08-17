import { initServer } from './server/index.js';
import * as cache from './cache/index.js';

type MakeEndpointsCacheS3Options = {
    method: 's3';
    bucket: string;
    region: string;
    endpoint?: string;
};

type MakeEndpointsCacheFileOptions = {
    method: 'file';
    dir: string;
    ttl: number;
};

type MakeEndpointsCacheMemoryOptions = {
    method: 'memory';
    ttl: number;
    maxItemCount: number;
};

type MakeEndpointsCacheNoneOptions = {
    method: 'none';
};

type MakeEndpointsOptions = {
    cache:
        | MakeEndpointsCacheS3Options
        | MakeEndpointsCacheFileOptions
        | MakeEndpointsCacheMemoryOptions
        | MakeEndpointsCacheNoneOptions;
};

function makeCache(options: MakeEndpointsOptions): cache.Cache {
    switch (options.cache.method) {
        case 'memory':
            return cache.memoryCache({
                ttl: options.cache.ttl,
                maxItemCount: options.cache.maxItemCount,
            });
        case 's3':
            return cache.s3Cache({
                bucket: options.cache.bucket,
                region: options.cache.region,
                endpoint: options.cache.endpoint ?? null,
            });
        case 'file':
            return cache.fileCache({
                dir: options.cache.dir,
                ttl: options.cache.ttl,
            });
        case 'none':
            return cache.noneCache();
    }
}

/**
 * initialize chiitiler endpoints: /tiles, /clip
 * @param options
 * @returns - { tiles, clip }
 */
function makeEndpoints(options: MakeEndpointsOptions) {
    // This function is used as library mode.
    // Because options[port,debug] and return value [app,start()] has meaning only in cli mode,
    // they aren't exposed in the library mode.
    const { tiles, clip } = initServer({
        port: 3000, // ignored
        debug: false, // ignored
        cache: makeCache(options),
    });

    return { tiles, clip };
}

export { makeEndpoints };
