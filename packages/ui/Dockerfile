FROM node:20-alpine AS builder
WORKDIR /app

# Copy package.json files
COPY packages/ui/package*.json ./packages/ui/

# Install dependencies in ui package
WORKDIR /app/packages/ui
RUN npm install

# Copy source files
WORKDIR /app
COPY packages/ui/ ./packages/ui/

# Build the project
WORKDIR /app/packages/ui
ARG NEXT_PUBLIC_API_URL
ARG NEXT_PUBLIC_WS_URL
ENV NEXT_PUBLIC_API_URL=${NEXT_PUBLIC_API_URL}
ENV NEXT_PUBLIC_WS_URL=${NEXT_PUBLIC_WS_URL}

RUN mkdir -p public
RUN npm run build

FROM node:20-alpine AS runner
WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000
ENV HOSTNAME=0.0.0.0

# Copy the standalone output. Next.js standalone nests under the original path,
# so server.js is at .next/standalone/packages/ui/server.js
COPY --from=builder /app/packages/ui/.next/standalone ./
COPY --from=builder /app/packages/ui/.next/static ./packages/ui/.next/static
COPY --from=builder /app/packages/ui/public ./packages/ui/public

EXPOSE 3000

HEALTHCHECK --interval=10s --timeout=3s --start-period=15s \
  CMD wget -qO- http://localhost:3000/ || exit 1

# The server.js is nested inside packages/ui/ in the standalone output
CMD ["node", "packages/ui/server.js"]
