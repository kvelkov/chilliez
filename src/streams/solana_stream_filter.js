/**
 * QuickNode Solana Arbitrage Stream Filter
 * Optimized for detecting DEX arbitrage opportunities in real-time
 * Compatible with all QuickNode Solana RPC methods and WebSocket streams
 */
function main(data) {
	try {
		// Extract the stream data - handles various QuickNode stream formats
		const streamData = data.stream || data.params?.result || data;

		// Target addresses to monitor (Solana public keys)
		const watchedAddresses = [
			'So11111111111111111111111111111111111111112', // SOL mint
			'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC mint
			'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT mint
			'9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools program
			'675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM program
			'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter program
		];

		// DEX program IDs to monitor (all major Solana DEXs)
		const dexPrograms = [
			'9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM', // Orca Whirlpools
			'675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8', // Raydium AMM
			'JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB',  // Jupiter Aggregator
			'MERLuDFBMmsHnsBPZw2sDQZHvXFMwp8EdjudcU2HKky',  // Meteora
			'2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c', // Lifinity
			'PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY',  // Phoenix
			'EhpbDwV1vhwtoSZtZpGwVo3F8wq8AQy5i3H5N8rYZMEZ', // OpenBook
			'srmqPiDuMHpTXLG3d7Fqh5YuqC8YPq1x5U8QXGfZF6f', // Serum (legacy)
		];

		// High-value tokens for arbitrage (prioritized by volume and liquidity)
		const arbitrageTokens = [
			'So11111111111111111111111111111111111111112', // SOL (native)
			'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC
			'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT
			'mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So',  // mSOL (Marinade)
			'7dHbWXmci3dT8UFYWYZweBLXgycu7Y3iL6trKn1Y7ARj', // stSOL (Lido)
			'bSo13r4TkiE4KumL71LsHTPpL2euBYLFx6h9HP3piy1',  // bSOL (BlazeStake)
			'J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn', // jitoSOL
			'DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263', // Bonk
			'hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux',  // HNT
		];

		// Create sets for efficient lookups
		const addressSet = new Set(watchedAddresses.map(addr => addr.toLowerCase()));
		const programSet = new Set(dexPrograms.map(addr => addr.toLowerCase()));

		let matchingTransactions = [];
		let matchingAccountChanges = [];
		let matchingInstructions = [];

		// Handle different stream data formats
		if (streamData.transaction) {
			// Single transaction format
			const result = processTransaction(streamData.transaction, addressSet, programSet);
			if (result.matches) {
				matchingTransactions.push(result.transaction);
				matchingInstructions.push(...result.instructions);
			}
		} else if (streamData.transactions) {
			// Multiple transactions format
			streamData.transactions.forEach(transaction => {
				const result = processTransaction(transaction, addressSet, programSet);
				if (result.matches) {
					matchingTransactions.push(result.transaction);
					matchingInstructions.push(...result.instructions);
				}
			});
		}

		// Handle account change notifications
		if (streamData.account) {
			// Single account change
			const accountChange = processAccountChange(streamData.account, addressSet, programSet);
			if (accountChange.matches) {
				matchingAccountChanges.push(accountChange.account);
			}
		} else if (streamData.accounts) {
			// Multiple account changes
			streamData.accounts.forEach(account => {
				const accountChange = processAccountChange(account, addressSet, programSet);
				if (accountChange.matches) {
					matchingAccountChanges.push(accountChange.account);
				}
			});
		}

		// Check if we have any matches
		if (matchingTransactions.length === 0 && 
			matchingAccountChanges.length === 0 && 
			matchingInstructions.length === 0) {
			return null;
		}

		return {
			timestamp: Date.now(),
			slot: streamData.slot || null,
			transactions: matchingTransactions,
			accountChanges: matchingAccountChanges,
			instructions: matchingInstructions,
			metadata: {
				totalTransactions: matchingTransactions.length,
				totalAccountChanges: matchingAccountChanges.length,
				totalInstructions: matchingInstructions.length,
			}
		};

	} catch (e) {
		return { 
			error: e.message,
			timestamp: Date.now(),
			stack: e.stack 
		};
	}
}

/**
 * Process a single transaction for matches
 */
function processTransaction(transaction, addressSet, programSet) {
	let matches = false;
	let matchingInstructions = [];

	try {
		// Check transaction accounts
		if (transaction.transaction && transaction.transaction.message) {
			const message = transaction.transaction.message;
			
			// Check account keys
			if (message.accountKeys) {
				const accountMatches = message.accountKeys.some(account => {
					const accountStr = typeof account === 'string' ? account : account.pubkey;
					return accountStr && addressSet.has(accountStr.toLowerCase());
				});
				
				if (accountMatches) {
					matches = true;
				}
			}

			// Check instructions for program matches
			if (message.instructions) {
				message.instructions.forEach((instruction, index) => {
					const programId = message.accountKeys[instruction.programIdIndex];
					const programIdStr = typeof programId === 'string' ? programId : programId.pubkey;
					
					if (programIdStr && programSet.has(programIdStr.toLowerCase())) {
						matches = true;
						matchingInstructions.push({
							index: index,
							programId: programIdStr,
							accounts: instruction.accounts,
							data: instruction.data,
							transactionSignature: transaction.transaction.signatures?.[0]
						});
					}
				});
			}
		}

		// Check meta information for account changes
		if (transaction.meta) {
			// Check pre/post token balances
			if (transaction.meta.preTokenBalances || transaction.meta.postTokenBalances) {
				const tokenBalanceMatches = checkTokenBalances(
					transaction.meta.preTokenBalances, 
					transaction.meta.postTokenBalances, 
					addressSet
				);
				if (tokenBalanceMatches) {
					matches = true;
				}
			}

			// Check inner instructions
			if (transaction.meta.innerInstructions) {
				transaction.meta.innerInstructions.forEach(innerInstGroup => {
					innerInstGroup.instructions.forEach((instruction, index) => {
						const programId = transaction.transaction.message.accountKeys[instruction.programIdIndex];
						const programIdStr = typeof programId === 'string' ? programId : programId.pubkey;
						
						if (programIdStr && programSet.has(programIdStr.toLowerCase())) {
							matches = true;
							matchingInstructions.push({
								index: `inner_${innerInstGroup.index}_${index}`,
								programId: programIdStr,
								accounts: instruction.accounts,
								data: instruction.data,
								isInner: true,
								transactionSignature: transaction.transaction.signatures?.[0]
							});
						}
					});
				});
			}
		}

	} catch (e) {
		console.error('Error processing transaction:', e);
	}

	return {
		matches,
		transaction: matches ? transaction : null,
		instructions: matchingInstructions
	};
}

/**
 * Process account change notifications
 */
function processAccountChange(account, addressSet, programSet) {
	let matches = false;

	try {
		// Check if the account address matches
		if (account.pubkey && addressSet.has(account.pubkey.toLowerCase())) {
			matches = true;
		}

		// Check if the account owner/program matches
		if (account.account && account.account.owner && 
			programSet.has(account.account.owner.toLowerCase())) {
			matches = true;
		}

		// Check for specific data patterns (e.g., pool state changes)
		if (account.account && account.account.data) {
			// Add custom logic here for specific account data patterns
			// For example, detecting Whirlpool state changes, etc.
		}

	} catch (e) {
		console.error('Error processing account change:', e);
	}

	return {
		matches,
		account: matches ? account : null
	};
}

/**
 * Check token balance changes for monitored addresses
 */
function checkTokenBalances(preBalances, postBalances, addressSet) {
	try {
		const allBalances = [...(preBalances || []), ...(postBalances || [])];
		
		return allBalances.some(balance => {
			return balance.mint && addressSet.has(balance.mint.toLowerCase());
		});
	} catch (e) {
		console.error('Error checking token balances:', e);
		return false;
	}
}

// Export for use in other modules
if (typeof module !== 'undefined' && module.exports) {
	module.exports = { main };
}
