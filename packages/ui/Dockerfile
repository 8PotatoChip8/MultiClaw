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
RUN mkdir -p public
RUN npm run build

FROM node:20-alpine AS runner
WORKDIR /app

# Copy necessary files from builder
COPY --from=builder /app/packages/ui/next.config.js ./
COPY --from=builder /app/packages/ui/public ./public
COPY --from=builder /app/packages/ui/.next/standalone ./
COPY --from=builder /app/packages/ui/.next/static ./.next/static

# Set Next.js environment variables to run in production mode
ENV NODE_ENV production
ENV PORT 3000

# We use host network mode in docker-compose, so expose just documents the port.
EXPOSE 3000

# Next.js standalone output creates a server.js file
CMD ["node", "server.js"]
