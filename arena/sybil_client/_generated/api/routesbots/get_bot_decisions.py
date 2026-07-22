from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.bot_decision_feed_response import BotDecisionFeedResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    *,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,
    market_id: int | Unset = UNSET,
    since: str | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["trader"] = trader

    params["market_id"] = market_id

    params["since"] = since


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/bots/decisions",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> ApiErrorResponse | BotDecisionFeedResponse | None:
    if response.status_code == 200:
        response_200 = BotDecisionFeedResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[ApiErrorResponse | BotDecisionFeedResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,
    market_id: int | Unset = UNSET,
    since: str | Unset = UNSET,

) -> Response[ApiErrorResponse | BotDecisionFeedResponse]:
    """ GET /v1/bots/decisions

     Public bot analytics backed by Arena's private typed read service. The Rust
    API owns the public route and contract, while Python owns its storage and
    query semantics.

    Args:
        limit (int | Unset):
        trader (str | Unset):
        market_id (int | Unset):
        since (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorResponse | BotDecisionFeedResponse]
     """


    kwargs = _get_kwargs(
        limit=limit,
trader=trader,
market_id=market_id,
since=since,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,
    market_id: int | Unset = UNSET,
    since: str | Unset = UNSET,

) -> ApiErrorResponse | BotDecisionFeedResponse | None:
    """ GET /v1/bots/decisions

     Public bot analytics backed by Arena's private typed read service. The Rust
    API owns the public route and contract, while Python owns its storage and
    query semantics.

    Args:
        limit (int | Unset):
        trader (str | Unset):
        market_id (int | Unset):
        since (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorResponse | BotDecisionFeedResponse
     """


    return sync_detailed(
        client=client,
limit=limit,
trader=trader,
market_id=market_id,
since=since,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,
    market_id: int | Unset = UNSET,
    since: str | Unset = UNSET,

) -> Response[ApiErrorResponse | BotDecisionFeedResponse]:
    """ GET /v1/bots/decisions

     Public bot analytics backed by Arena's private typed read service. The Rust
    API owns the public route and contract, while Python owns its storage and
    query semantics.

    Args:
        limit (int | Unset):
        trader (str | Unset):
        market_id (int | Unset):
        since (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ApiErrorResponse | BotDecisionFeedResponse]
     """


    kwargs = _get_kwargs(
        limit=limit,
trader=trader,
market_id=market_id,
since=since,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,
    market_id: int | Unset = UNSET,
    since: str | Unset = UNSET,

) -> ApiErrorResponse | BotDecisionFeedResponse | None:
    """ GET /v1/bots/decisions

     Public bot analytics backed by Arena's private typed read service. The Rust
    API owns the public route and contract, while Python owns its storage and
    query semantics.

    Args:
        limit (int | Unset):
        trader (str | Unset):
        market_id (int | Unset):
        since (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ApiErrorResponse | BotDecisionFeedResponse
     """


    return (await asyncio_detailed(
        client=client,
limit=limit,
trader=trader,
market_id=market_id,
since=since,

    )).parsed
