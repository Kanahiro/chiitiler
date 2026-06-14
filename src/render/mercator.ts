const DEG2RAD = Math.PI / 180;

// lon/lat <-> world coords normalized to [0, 1] (Web Mercator).
// north is small y; antimeridian is x=0/1.
const mercX = (lon: number) => (lon + 180) / 360;
const mercY = (lat: number) =>
    0.5 - Math.log(Math.tan(Math.PI / 4 + (lat * DEG2RAD) / 2)) / (2 * Math.PI);
const invMercX = (x: number) => x * 360 - 180;
const invMercY = (y: number) =>
    (2 * Math.atan(Math.exp((0.5 - y) * 2 * Math.PI)) - Math.PI / 2) / DEG2RAD;

export { mercX, mercY, invMercX, invMercY };
