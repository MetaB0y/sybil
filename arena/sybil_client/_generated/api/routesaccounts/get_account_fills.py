from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.account_fill_page_response import AccountFillPageResponse
from ...models.api_error_response import ApiErrorResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    id: int,
    *,
    market_id: int | Unset = UNSET,
    after: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["market_id"] = market_id

    params["after"] = after

    params["limit"] = limit


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/accounts/{id}/fills".format(id=quote(str(id), safe=""),),
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> AccountFillPageResponse | Any | ApiErrorResponse | None:
    if response.status_code == 200:
        response_200 = AccountFillPageResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 401:
        response_401 = cast(Any, None)
        return response_401

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if response.status_code == 503:
        response_503 = cast(Any, None)
        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[AccountFillPageResponse | Any | ApiErrorResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    market_id: int | Unset = UNSET,
    after: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[AccountFillPageResponse | Any | ApiErrorResponse]:
    """ GET /v1/accounts/{id}/fills

    Args:
        id (int):
        market_id (int | Unset):
        after (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountFillPageResponse | Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        id=id,
market_id=market_id,
after=after,
limit=limit,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient,
    market_id: int | Unset = UNSET,
    after: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> AccountFillPageResponse | Any | ApiErrorResponse | None:
    """ GET /v1/accounts/{id}/fills

    Args:
        id (int):
        market_id (int | Unset):
        after (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountFillPageResponse | Any | ApiErrorResponse
     """


    return sync_detailed(
        id=id,
client=client,
market_id=market_id,
after=after,
limit=limit,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    market_id: int | Unset = UNSET,
    after: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[AccountFillPageResponse | Any | ApiErrorResponse]:
    """ GET /v1/accounts/{id}/fills

    Args:
        id (int):
        market_id (int | Unset):
        after (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountFillPageResponse | Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        id=id,
market_id=market_id,
after=after,
limit=limit,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,
    market_id: int | Unset = UNSET,
    after: str | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> AccountFillPageResponse | Any | ApiErrorResponse | None:
    """ GET /v1/accounts/{id}/fills

    Args:
        id (int):
        market_id (int | Unset):
        after (str | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountFillPageResponse | Any | ApiErrorResponse
     """


    return (await asyncio_detailed(
        id=id,
client=client,
market_id=market_id,
after=after,
limit=limit,

    )).parsed
