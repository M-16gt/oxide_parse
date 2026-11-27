from oxide_parse import Plan, T, compile
from pathlib import Path
from tempfile import TemporaryDirectory

plan = compile({"title": T.css("h1").text(), "links": T.css("a").map(T.attr("href"))})

text = plan.to_json(indent=2)          # save the compiled tree...
with TemporaryDirectory() as directory:
    path = Path(directory) / "plan.json"
    path.write_text(text, encoding="utf-8")
    loaded = Plan.from_file(str(path))
print(loaded.extract("<h1>Hi</h1><a href='/x'>x</a>"))

print(loaded.to_python())              # IR -> DSL source code
