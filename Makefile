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
	  'make production-up     Start immutable production images'

install:
	./manage install

dev:
	./manage dev

down:
	./manage down

logs:
	./manage logs

frontend-dev:
	./manage frontend-dev

frontend-build:
	./manage frontend-build

backend-fmt:
	./manage backend-fmt

backend-lint:
	./manage backend-lint

backend-test:
	./manage backend-test

check:
	./manage check

compose-config:
	./manage compose-config

production-pull:
	./manage production-pull

production-up:
	./manage production-up

production-down:
	./manage production-down

preflight:
	./manage preflight

backup:
	./manage backup

restore-rehearsal:
	./manage restore-rehearsal
