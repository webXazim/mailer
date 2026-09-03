.PHONY: help install dev down logs frontend-dev frontend-build backend-fmt backend-lint backend-test check compose-config production-pull production-up production-down preflight backup restore-rehearsal

help:
	@printf '%s\n' \
	  'make dev               Build and start the complete local stack' \
	  'make install           Install frontend dependencies' \
	  'make down              Stop the local stack' \
	  'make logs              Follow all service logs' \
	  'make frontend-dev      Run the Vite development server' \
	  'make frontend-build    Build the frontend' \
	  'make backend-test      Run backend tests' \
	  'make check             Verify frontend and backend' \
	  'make compose-config    Validate local Compose configuration' \
	  'make preflight         Validate production configuration' \
	  'make production-init   Create .env with generated local secrets' \
	  'make deploy            Build and deploy on the VPS' \
	  'make production-up     Build and start VPS services'

install:
	sh ./manage install

dev:
	sh ./manage dev

down:
	sh ./manage down

logs:
	sh ./manage logs

frontend-dev:
	sh ./manage frontend-dev

frontend-build:
	sh ./manage frontend-build

backend-fmt:
	sh ./manage backend-fmt

backend-lint:
	sh ./manage backend-lint

backend-test:
	sh ./manage backend-test

check:
	sh ./manage check

compose-config:
	sh ./manage compose-config

production-pull:
	sh ./manage production-pull

production-up:
	sh ./manage production-up

production-down:
	sh ./manage production-down

preflight:
	sh ./manage preflight

backup:
	sh ./manage backup

restore-rehearsal:
	sh ./manage restore-rehearsal

.PHONY: deploy production-init production-logs production-status healthcheck account-email-status
deploy production-init production-logs production-status healthcheck:
	sh ./manage $@

account-email-status:
	sh ./manage $@ "$(EMAIL)"
