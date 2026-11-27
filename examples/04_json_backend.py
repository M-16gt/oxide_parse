from oxide_parse import T, compile

plan = compile({
    "names": T["users"].map(T["name"].upper()),
    "n_adults": T["users"].filter(T["age"].ne(3)).count(),
    "first_city": T["users"][0]["address"]["city"].default("unknown"),
}, backend="json")

print(plan.extract({"users": [{"name": "ann", "age": 30, "address": {"city": "Riga"}}, {"name": "bob", "age": 3}]}))
