# Transfers Indexer

The Transfers Indexer is a powerful tool designed for archiving MAS token transfers from Externally Owned Accounts (EOA) on the Massa blockchain. It utilizes the API exposed by the [Massa node](https://github.com/massalabs/massa) for data collection and stores this information in a MySQL database.

## Prerequisites

- **Massa Node with Execution Trace**: You must have access to a Massa node compiled with the `execution-trace` feature or run the source code with `cargo run -r --features execution-trace`.
- **MySQL Database**: A MySQL server is necessary for data storage. If you do not have MySQL installed, you can deploy a containerized version using the provided Docker Compose file with the command `docker-compose up -d`.

## Setup Instructions

To start the indexer, execute:

```sh
cargo run -r
```

This command launches the indexing process, capturing and storing transfer data from the Massa blockchain.

## API Documentation

For a detailed overview of the API endpoints, including query parameters, request examples, and response structures, please refer to our [Swagger API documentation](transfers-indexer.yml).


## FAQ

### Using a Remote Massa Node

To configure the indexer to connect to a remote Massa node, update the `.env` file with the remote node's IP and public port:

```toml
MASSA_NODE_API_IP="remote_node_ip"
MASSA_NODE_API_PUBLIC_PORT="remote_node_port"
```

### Configuring the API Listener

To change the network interface or port the API listens to, modify the `INDEXER_API` value in the `.env` file:

```toml
INDEXER_API="desired_ip:desired_port"
```

### Using a Remote MySQL Database

If you prefer using a remote MySQL database instead of a local or Docker-based instance, specify the database URL in the `.env` file:

```toml
DATABASE_URL='mysql://user:password@remote_host:3306/massa_transfers'
```

### Changing Database Credentials

To change the database credentials, adjust the `DATABASE_URL` in the `.env` file to reflect the new user and password:

```toml
DATABASE_URL='mysql://new_user:new_password@localhost:3306/massa_transfers'
```

This configuration allows the indexer to connect to the specified MySQL database using the provided credentials.