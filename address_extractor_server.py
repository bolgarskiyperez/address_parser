from flask import Flask, request, jsonify
import spacy

nlp = spacy.load("ru_core_news_lg")
app = Flask(__name__)

@app.route("/extract", methods=["POST"])
def extract_addresses():
    data = request.get_json()
    text = data.get("text", "")

    doc = nlp(text)
    addresses = []

    for ent in doc.ents:
        if ent.label_ in ("LOC", "GPE", "FAC"):  # Местоположения, города, строения
            addresses.append(ent.text)

    return jsonify({
        "addresses": list(set(addresses))  # Удаляем дубли
    })

if __name__ == "__main__":
    app.run(host="127.0.0.1", port=5000)
