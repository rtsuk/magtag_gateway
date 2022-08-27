    cargo run -- --line data/NJD_during_03_linescore.json --next data/NJD_before.json


    gcloud artifacts repositories create magtag-gateway --repository-format=docker --location=us-west1 --description="MagTag Gateway"
    gcloud builds submit --tag us-west1-docker.pkg.dev/tsuk-331415/magtag-gateway/magtag-gateway-image:latest --timeout 1h
