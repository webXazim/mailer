FROM python:3.12-slim-bookworm
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt \
    && groupadd --system --gid 10001 mailer \
    && useradd --system --uid 10001 --gid 10001 mailer
COPY gateway.py .
USER mailer
EXPOSE 465 587
ENTRYPOINT ["python", "-u", "/app/gateway.py"]
