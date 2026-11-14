"""python -m unittest discover crates/python/tests (with the wheel installed), from the repository root."""

import json
import pathlib
import unittest

import ml2drender

ROOT = pathlib.Path(__file__).resolve().parents[3]

STYLE = json.dumps({
    "version": 8,
    "sources": {"dir": {"type": "vector", "tiles": ["test://{z}/{x}/{y}.pbf"], "maxzoom": 6}},
    "layers": [
        {"id": "bg", "type": "background", "paint": {"background-color": "#ffffff"}},
        {"id": "dir", "type": "circle", "source": "dir", "source-layer": "P2921", "paint": {"circle-color": "#ff0000"}},
    ],
})


def load(url):
    path = ROOT / "localdata/tiles" / url.removeprefix("test://")
    return path.read_bytes() if path.exists() else None


class RenderTest(unittest.TestCase):
    def test_render_tile(self):
        view = ml2drender.View.tile(5, 28, 12, 256)
        rgba = ml2drender.render(STYLE, view, load)
        width, height = view.size
        self.assertEqual(len(rgba), width * height * 4)
        pixels = {bytes(rgba[i : i + 4]) for i in range(0, len(rgba), 4)}
        self.assertIn(bytes([255, 255, 255, 255]), pixels)  # background
        self.assertIn(bytes([255, 0, 0, 255]), pixels)  # circles from the tile
        png = ml2drender.encode_png(rgba, width, height)
        self.assertEqual(png[:4], b"\x89PNG")
        self.assertEqual(ml2drender.encode_jpeg(rgba, width, height)[:2], b"\xff\xd8")

    def test_step_by_step(self):
        r = ml2drender.Renderer(STYLE, ml2drender.View((140.0, 37.0), 4.0, 64, 32))
        urls = r.needs()
        self.assertTrue(urls)
        for url in urls:
            r.provide(url, load(url))
        self.assertEqual(r.needs(), [])
        self.assertEqual(len(r.render()), 64 * 32 * 4)

    def test_errors(self):
        with self.assertRaises(RuntimeError):
            ml2drender.Renderer("{", ml2drender.View.tile(0, 0, 0))

        def broken(url):
            raise OSError("offline")

        with self.assertRaises(OSError):
            ml2drender.render(STYLE, ml2drender.View.tile(5, 28, 12), broken)


if __name__ == "__main__":
    unittest.main()
